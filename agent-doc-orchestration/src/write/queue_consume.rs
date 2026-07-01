//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_document::queue_projection::{IN_PROGRESS_MARKER, strip_priority_markers};
use agent_doc_queue::{
    queue_consume::{
        annotate_newly_struck_free_text_heads, first_n_queue_prompt_texts,
        mark_entries_completed_by_done_ids, mark_first_matching_prompts_completed_by_texts,
        normalized_done_id_bag, queue_consume_count_for_done_ids,
        queue_prompt_texts_match_for_consumption,
    },
    queue_directive::topic_resolves_to_exact_id,
    queue_heads::{
        active_queue_head_text, queue_head_has_explicit_completion_signal,
        queue_head_matches_done_ids,
    },
    queue_response::{
        embed_consumed_prompt_in_response, first_nonempty_line,
        free_text_head_answered_by_response, free_text_head_present_in_baseline,
        head_carries_in_progress_marker, head_id_names_tracked_directive_item, normalize_done_id,
        normalize_queue_prompt_text, queue_head_is_bare_do_directive,
        queue_head_is_free_text_prompt, queue_prompt_done_id, queue_prompt_text_is_free_text,
        queue_prompt_text_matches, response_explicitly_targets_queue_head, response_heading_topic,
    },
};

pub(crate) struct QueueConsumptionPlan {
    pub(crate) consumed_text: String,
    pub(crate) consumed_texts: Vec<String>,
    pub(crate) node_ops: Vec<IpcNodeOp>,
    pub(crate) remaining: usize,
    pub(crate) drained: bool,
    pub(crate) auto: bool,
    pub(crate) new_document: String,
    pub(crate) new_snapshot: String,
    pub(crate) save_snapshot: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpcNodeOp {
    pub component: String,
    pub node_id: String,
    pub op: String,
}

impl IpcNodeOp {
    fn consume(component: &str, node_id: String) -> Self {
        Self {
            component: component.to_string(),
            node_id,
            op: "consume".to_string(),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "component": self.component,
            "node_id": self.node_id,
            "op": self.op,
        })
    }
}

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
pub fn consume_queue_prompt(file: &Path) -> Result<bool> {
    Ok(consume_queue_prompt_with_outcome(file)?.is_some())
}

pub fn consume_queue_prompt_with_outcome(file: &Path) -> Result<Option<QueueConsumptionOutcome>> {
    consume_queue_prompts_with_outcome(file, &[], false)
}

pub fn consume_queue_prompts_for_done_ids_with_outcome(
    file: &Path,
    done_ids: &[String],
) -> Result<Option<QueueConsumptionOutcome>> {
    consume_queue_prompts_with_outcome(file, done_ids, false)
}

pub fn consume_queue_prompts_for_done_ids_force_disk_with_outcome(
    file: &Path,
    done_ids: &[String],
) -> Result<Option<QueueConsumptionOutcome>> {
    consume_queue_prompts_with_outcome(file, done_ids, true)
}

/// Strike the active queue head, **skipping the visible-write idle guard**, for
/// the repair recovery path (`#repair-strike-consumed-head`). Repair already
/// writes the recovered response straight to disk (bypassing IPC/IDE), so the
/// matching head strike must also bypass the guard — otherwise a live IDE buffer
/// would block the strike and leave the answered free-text head live for
/// preflight to re-present. Callers must scope this to heads the recovered
/// response actually answered.
pub fn consume_queue_prompt_force_disk(file: &Path) -> Result<Option<QueueConsumptionOutcome>> {
    consume_queue_prompts_with_outcome(file, &[], true)
}

pub(crate) fn consume_queue_prompts_with_outcome(
    file: &Path,
    done_ids: &[String],
    skip_visible_guard: bool,
) -> Result<Option<QueueConsumptionOutcome>> {
    // Hold the document lock for the entire read-parse-write cycle to prevent
    // concurrent edits from invalidating parsed offsets (TOCTOU fix).
    let _lock = acquire_doc_lock(file)?;
    let content =
        std::fs::read_to_string(file).context("queue consume: failed to read document")?;
    let Some(plan) = plan_queue_prompt_consumption(file, &content, done_ids)? else {
        return Ok(None);
    };

    record_queue_consumption_proofs(file, &plan, QueueConsumptionProofStage::BeforeMutation)?;

    // `#fcc0`: converge the queue-consume write through the editor IPC when a JB
    // listener is active (no `File Cache Conflict` dialog); fall back to the
    // guarded disk write otherwise. The force-disk repair path keeps its raw
    // bypass — it deliberately skips IPC/IDE and the visible-write guard.
    if skip_visible_guard {
        atomic_write(file, &plan.new_document)
            .context("queue consume: failed to write document")?;
    } else {
        converge_document_or_disk(file, &plan.new_document, &content, "queue_consume")
            .context("queue consume: failed to write document")?;
    }
    if plan.save_snapshot {
        snapshot::save(file, &plan.new_snapshot)?;
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
    if let Err(err) = crate::owner_pane_wedge_counter::clear(file) {
        eprintln!(
            "[recguard-wedge] WARNING: failed to clear wedge counter for {}: {}",
            file.display(),
            err
        );
    }

    Ok(Some(outcome))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueueConsumptionProofStage {
    BeforeMutation,
    AfterMutation,
}

pub(crate) fn record_queue_consumption_proofs(
    file: &Path,
    plan: &QueueConsumptionPlan,
    stage: QueueConsumptionProofStage,
) -> Result<()> {
    let canonical = file
        .canonicalize()
        .with_context(|| format!("queue consume: failed to canonicalize {}", file.display()))?;
    let Some(project_root) = agent_doc_fs::find_project_root(&canonical) else {
        eprintln!(
            "[queue] warning: proof ledger unavailable for {}: project root not found",
            file.display()
        );
        return Ok(());
    };
    let document_hash = queue_state_document_hash(&canonical);
    for (index, consumed_text) in plan.consumed_texts.iter().enumerate() {
        let content_hash = agent_doc_hash::content_hash(consumed_text);
        let node_id = plan
            .node_ops
            .get(index)
            .map(|op| op.node_id.as_str())
            .unwrap_or("<missing-node>");
        let operation_id = format!("queue_head:{node_id}:{index}");
        let (outcome, proof_kind, proof) = match stage {
            QueueConsumptionProofStage::BeforeMutation => (
                crate::flow::proof_ledger::ProofOutcome::Recorded,
                crate::flow::proof_ledger::ProofEvidenceKind::QueueHeadIdentity,
                format!(
                    "phase=before_mutation node_id={} index={} consumed_count={} text_hash={} text={:?}",
                    node_id,
                    index,
                    plan.consumed_texts.len(),
                    content_hash,
                    consumed_text
                ),
            ),
            QueueConsumptionProofStage::AfterMutation => (
                crate::flow::proof_ledger::ProofOutcome::Consumed,
                crate::flow::proof_ledger::ProofEvidenceKind::WriteResult,
                format!(
                    "phase=after_mutation node_id={} index={} remaining={} drained={} auto={} save_snapshot={}",
                    node_id, index, plan.remaining, plan.drained, plan.auto, plan.save_snapshot
                ),
            ),
        };
        let record = crate::flow::proof_ledger::OperationProofRecord::new(
            crate::flow::proof_ledger::OperationProofInput {
                operation_id,
                operation_kind: crate::flow::proof_ledger::ProofOperationKind::QueueHead,
                outcome,
                subject_id: Some(node_id.to_string()),
                content_hash,
                proof_kind,
                proof,
                recorded_at_ms: now_millis(),
            },
        )?;
        let path =
            crate::flow::proof_ledger::append_operation_proof(&project_root, &canonical, &record)?;
        crate::ops_log::log_op(
            file,
            &format!(
                "queue_consume_proof_recorded file={} stage={:?} operation_id={} ledger={}",
                file.display(),
                stage,
                record.operation_id,
                path.display()
            ),
        );
        record_queue_consumption_state_event(QueueConsumptionStateEvent {
            file,
            project_root: &project_root,
            document_hash: &document_hash,
            node_id,
            index,
            consumed_text,
            content_hash: &record.content_hash,
            stage,
        })?;
    }
    if stage == QueueConsumptionProofStage::AfterMutation && !plan.drained {
        record_next_queue_head_selected_state(
            file,
            &project_root,
            &document_hash,
            &plan.new_document,
        )?;
    }
    Ok(())
}

fn queue_state_document_hash(file: &Path) -> String {
    agent_doc_hash::document_id_for_path(file)
}

struct QueueConsumptionStateEvent<'a> {
    file: &'a Path,
    project_root: &'a Path,
    document_hash: &'a str,
    node_id: &'a str,
    index: usize,
    consumed_text: &'a str,
    content_hash: &'a str,
    stage: QueueConsumptionProofStage,
}

fn record_queue_consumption_state_event(args: QueueConsumptionStateEvent<'_>) -> Result<()> {
    let QueueConsumptionStateEvent {
        file,
        project_root,
        document_hash,
        node_id,
        index,
        consumed_text,
        content_hash,
        stage,
    } = args;
    let backlog_id = queue_prompt_done_id(consumed_text);
    let (event_id, fact) = match stage {
        QueueConsumptionProofStage::BeforeMutation => (
            format!("queue-head-selected:{document_hash}:{node_id}:{index}:{content_hash}"),
            crate::state_backbone::StateFact::QueueHeadSelected {
                document_hash: document_hash.to_string(),
                node_key: node_id.to_string(),
                backlog_id,
                prompt_text: Some(consumed_text.to_string()),
                drainable: true,
                hosting_epoch: None,
            },
        ),
        QueueConsumptionProofStage::AfterMutation => (
            format!("queue-head-completed:{document_hash}:{node_id}:{index}:{content_hash}"),
            crate::state_backbone::StateFact::QueueHeadCompleted {
                document_hash: document_hash.to_string(),
                node_key: node_id.to_string(),
                backlog_id,
                hosting_epoch: None,
            },
        ),
    };
    let event = crate::state_backbone::StateEvent::new(event_id, fact);
    let inserted = crate::project_controller::append_state_event(project_root, &event)?;
    crate::ops_log::log_op(
        file,
        &format!(
            "queue_consume_state_event_recorded file={} stage={:?} event_id={} inserted={} document_hash={} node_id={}",
            file.display(),
            stage,
            event.event_id,
            inserted,
            document_hash,
            node_id
        ),
    );
    Ok(())
}

fn record_next_queue_head_selected_state(
    file: &Path,
    project_root: &Path,
    document_hash: &str,
    content: &str,
) -> Result<()> {
    let Some((node_key, head_text, stop_fence_at_head)) = next_queue_head_selection(content)?
    else {
        return Ok(());
    };
    let content_hash = agent_doc_hash::content_hash(&head_text);
    let drainable = !stop_fence_at_head
        && agent_doc_queue::queue_continuation::live_drainable_continuation_head(
            content,
            agent_doc_queue::queue_continuation::DrainScope::Supervisor,
        )
        .is_some();
    let selected_event = crate::state_backbone::StateEvent::new(
        format!("queue-head-selected:{document_hash}:{node_key}:0:{content_hash}"),
        crate::state_backbone::StateFact::QueueHeadSelected {
            document_hash: document_hash.to_string(),
            node_key: node_key.clone(),
            backlog_id: queue_prompt_done_id(&head_text),
            prompt_text: Some(head_text.clone()),
            drainable,
            hosting_epoch: None,
        },
    );
    let inserted = crate::project_controller::append_state_event(project_root, &selected_event)?;
    crate::ops_log::log_op(
        file,
        &format!(
            "queue_next_selected_state_event_recorded file={} event_id={} inserted={} document_hash={} node_id={} drainable={}",
            file.display(),
            selected_event.event_id,
            inserted,
            document_hash,
            node_key,
            drainable
        ),
    );
    if stop_fence_at_head {
        let reason = "stop_fence";
        let reason_hash = agent_doc_hash::content_hash(reason);
        let deferred_event = crate::state_backbone::StateEvent::new(
            format!(
                "queue-head-deferred:{document_hash}:{node_key}:0:{reason_hash}:{content_hash}"
            ),
            crate::state_backbone::StateFact::QueueHeadDeferred {
                document_hash: document_hash.to_string(),
                node_key: node_key.clone(),
                reason: reason.to_string(),
                hosting_epoch: None,
            },
        );
        let deferred_inserted =
            crate::project_controller::append_state_event(project_root, &deferred_event)?;
        crate::ops_log::log_op(
            file,
            &format!(
                "queue_next_deferred_state_event_recorded file={} event_id={} inserted={} document_hash={} node_id={} reason={}",
                file.display(),
                deferred_event.event_id,
                deferred_inserted,
                document_hash,
                node_key,
                reason
            ),
        );
    }
    Ok(())
}

fn next_queue_head_selection(content: &str) -> Result<Option<(String, String, bool)>> {
    let components = element::parse(content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(None);
    };
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = agent_doc_queue::document_queue::parse(body)
        .context("queue consume: failed to parse next queue head")?;
    let stop_fence_at_head = agent_doc_queue::document_queue::has_stop_fence_at_head(&entries);
    let Some(head_text) = first_n_queue_prompt_texts(&entries, 1).into_iter().next() else {
        return Ok(None);
    };
    let Some(node_key) = queue_prompt_node_keys_for_count(content, 1)?
        .keys
        .into_iter()
        .next()
    else {
        return Ok(None);
    };
    Ok(Some((node_key, head_text, stop_fence_at_head)))
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

pub fn should_consume_queue_prompt_for_diff(file: &Path, diff_text: Option<&str>) -> Result<bool> {
    let content =
        std::fs::read_to_string(file).context("queue consume guard: failed to read document")?;
    should_consume_queue_prompt_for_diff_content(file, &content, diff_text)
}

pub(crate) fn should_consume_queue_prompt_for_write(
    file: &Path,
    baseline: Option<&str>,
    current_content: &str,
    completion_ids: &[String],
) -> Result<bool> {
    // An explicit closeout signal naming the queue head authorizes consumption
    // regardless of any pending mutations bundled into the same diff
    // (#pending-add-suppresses-queue-consume). Check it FIRST so a bundled
    // `--pending-add` cannot make the diff-based check below emit a misleading
    // "active prompt differs from queue head" diagnostic for a turn that does
    // in fact complete the head.
    if queue_head_matches_done_ids(current_content, completion_ids)? {
        return Ok(true);
    }
    let Some(base) = baseline else {
        return Ok(false);
    };
    let base_norm = agent_doc_diff::strip_comments(&strip_boundary_for_dedup(base));
    let current_norm = agent_doc_diff::strip_comments(&strip_boundary_for_dedup(current_content));
    let diff_text = agent_doc_diff::unified_diff_from_contents(&base_norm, &current_norm);
    should_consume_queue_prompt_for_diff_content(file, current_content, diff_text.as_deref())
}

pub(crate) fn queue_skip_diagnostic_for_file(file: &Path) -> Result<String> {
    let content =
        std::fs::read_to_string(file).context("queue skip diagnostic: failed to read document")?;
    agent_doc_queue::queue_heads::queue_skip_diagnostic_for_content(&content)
}

pub(crate) fn should_consume_queue_prompt_for_diff_content(
    file: &Path,
    content: &str,
    diff_text: Option<&str>,
) -> Result<bool> {
    let Some(queue_head) = active_queue_head_text(content)? else {
        return Ok(true);
    };
    let Some(diff_text) = diff_text else {
        return Ok(false);
    };
    let prompt_changes: Vec<_> = agent_doc_diff::classify_prompt_bearing_changes(diff_text)
        .into_iter()
        .filter(|change| {
            matches!(
                change.kind,
                agent_doc_diff::PromptBearingChangeKind::PromptTarget
                    | agent_doc_diff::PromptBearingChangeKind::ContentEdit
            )
        })
        .collect();
    if agent_doc_diff::detect_queue_trigger(diff_text) {
        return Ok(true);
    }
    if prompt_changes
        .iter()
        .any(|change| queue_prompt_text_matches(&change.text, &queue_head))
    {
        return Ok(true);
    }

    // Not a user-facing failure on its own: the caller still has explicit
    // completion-signal fallbacks (`--done`/`--pending-gate`/`--review-resolve`/
    // `--pending-edit`, synthetic-head heading match). Only the caller's final
    // "skipped consumption" line is the authoritative skip signal, so record this
    // detail to ops_log instead of stderr to avoid a false-alarm during a turn
    // that ultimately consumes the head (#pending-add-suppresses-queue-consume).
    // The {:?} quoting on prompt_changes/queue_head is load-bearing: the
    // gate-verify scan excludes double-quoted spans so this embedded document
    // prose cannot prove a gated review item (#gng8).
    crate::ops_log::log_op(
        file,
        &format!(
            "queue_diff_active_prompt_differs file={} prompt_changes={:?} queue_head={:?}",
            file.display(),
            prompt_changes
                .iter()
                .map(|change| change.text.as_str())
                .collect::<Vec<_>>(),
            queue_head
        ),
    );
    Ok(false)
}

/// True when this cycle's diff introduced a prompt-bearing exchange change (a
/// new or edited user prompt) that does NOT match the active queue head — i.e.
/// the response answered *foreign* exchange work. Used to keep a free-text queue
/// head queued when the cycle was driven by an unrelated new exchange prompt
/// rather than by draining the head (#queue-head-struck-on-foreign-exchange-answer).
///
/// A legitimate free-text-head drain has no such foreign prompt-bearing change
/// (the head itself was already in the baseline queue, and the only addition is
/// this cycle's `### Re:` response, which is not classified as a prompt), so this
/// returns false and the head is allowed to drain.
pub(crate) fn cycle_answered_foreign_exchange_prompt(
    baseline: Option<&str>,
    current_content: &str,
    queue_head: &str,
) -> bool {
    let Some(base) = baseline else {
        return false;
    };
    let base_norm = agent_doc_diff::strip_comments(&strip_boundary_for_dedup(base));
    let current_norm = agent_doc_diff::strip_comments(&strip_boundary_for_dedup(current_content));
    let Some(diff_text) = agent_doc_diff::unified_diff_from_contents(&base_norm, &current_norm)
    else {
        return false;
    };
    // A foreign exchange prompt is a user-prompt line (`❯ …`) genuinely NEW this
    // cycle whose text is not the queue head. The bug shape is a foreign prompt
    // that WAS answered this cycle, and `classify_prompt_bearing_changes`
    // suppresses prompts already answered by an adjacent response — so scan the
    // raw added lines for the canonical `❯` user-prompt marker instead of the
    // suppressed classifier.
    //
    // #free-text-head-consume-genuine-not-struck: the unified diff is computed
    // against the *normalized snapshot* baseline, but `current_content` is the
    // *live* working-tree/editor buffer. The buffer preserves `❯` prompt
    // prefixes on already-answered prompts that the snapshot normalized to the
    // bare form (CLAUDE.md "committed exchange-only prompt-prefix normalization
    // on already-answered prompts"). A pure `do x` → `❯ do x` prefix flip then
    // shows as an added `+❯ …` line and was wrongly read as a NEW foreign
    // prompt, blocking the free-text head strike and stalling the auto-loop. So
    // a `❯` added line counts as foreign only when its normalized text is absent
    // from the baseline entirely — a genuine new prompt, not a prefix flip on a
    // prompt that already existed (in either `❯ X` or bare `X` form) at baseline.
    let baseline_prompt_texts: std::collections::HashSet<String> = base_norm
        .lines()
        .map(|line| normalize_queue_prompt_text(line.trim().trim_start_matches('❯').trim()))
        .filter(|text| !text.is_empty())
        .collect();
    let debug = std::env::var("AGENT_DOC_DEBUG_QUEUE_CONSUME").is_ok();
    diff_text.lines().any(|line| {
        let Some(added) = line.strip_prefix('+') else {
            return false;
        };
        if added.starts_with("++") {
            return false; // unified-diff `+++` file header, not content
        }
        let Some(prompt) = added.trim().strip_prefix('❯') else {
            return false;
        };
        let prompt = prompt.trim();
        if prompt.is_empty() || queue_prompt_text_matches(prompt, queue_head) {
            return false;
        }
        // Skip prefix-normalization artifacts: the prompt text already existed in
        // the baseline (bare or `❯`-prefixed), so it is not new this cycle.
        if baseline_prompt_texts.contains(&normalize_queue_prompt_text(prompt)) {
            if debug {
                eprintln!(
                    "[queue-consume] ❯ added line is a prefix-flip on an existing baseline prompt, not foreign: {prompt:?}"
                );
            }
            return false;
        }
        if debug {
            eprintln!(
                "[queue-consume] foreign ❯ prompt added this cycle (blocks free-text head strike): {prompt:?} (head={queue_head:?})"
            );
        }
        true
    })
}

/// Return the id of the pre-commit queue head when this turn targeted that
/// exact head through the prompt diff or response heading. The queue-consume
/// planner rechecks the live head later, so this id cannot authorize striking a
/// different head that was reordered into first position after the decision.
pub(crate) fn queue_targeted_completion_id_for_current_head(
    file: &Path,
    baseline: Option<&str>,
    current_content: &str,
    response_body: &str,
    pending_done: &[String],
) -> Result<Option<String>> {
    if queue_head_is_free_text_prompt(current_content)? {
        return Ok(None);
    }
    let Some(queue_head) = active_queue_head_text(current_content)? else {
        return Ok(None);
    };
    let Some(head_id) = queue_prompt_done_id(&queue_head) else {
        return Ok(None);
    };
    if !response_body.trim().is_empty()
        && response_explicitly_targets_queue_head(response_body, &queue_head)
    {
        return Ok(Some(head_id));
    }
    if should_consume_queue_prompt_for_write(file, baseline, current_content, pending_done)? {
        return Ok(Some(head_id));
    }
    Ok(None)
}

pub(crate) fn queue_diff_completion_id_for_current_head(
    file: &Path,
    current_content: &str,
    diff_text: &str,
) -> Result<Option<String>> {
    if queue_head_is_free_text_prompt(current_content)? {
        return Ok(None);
    }
    let Some(queue_head) = active_queue_head_text(current_content)? else {
        return Ok(None);
    };
    let Some(head_id) = queue_prompt_done_id(&queue_head) else {
        return Ok(None);
    };
    if should_consume_queue_prompt_for_diff_content(file, current_content, Some(diff_text))? {
        return Ok(Some(head_id));
    }
    Ok(None)
}

pub fn response_explicitly_targets_active_queue_head(file: &Path, response: &str) -> Result<bool> {
    let content =
        std::fs::read_to_string(file).context("queue consume guard: failed to read document")?;
    let Some(queue_head) = active_queue_head_text(&content)? else {
        return Ok(false);
    };
    Ok(response_explicitly_targets_queue_head(
        response,
        &queue_head,
    ))
}

/// True when this cycle's captured response heading targets EXACTLY the active
/// queue head's id and that head is a *synthetic/preset* prompt rather than an
/// id-backed directive (#queue-head-consume-on-topic-id-regression / #zwn5).
///
/// Synthetic queue prompts — a preset expansion or a natural-language prompt
/// carrying a trailing `#preset` id — are completed by the response itself, so a
/// `### Re: #<id>` heading that resolves to exactly that id is a genuine
/// completion signal. Id-backed directives still require an explicit closeout
/// flag (#queue-strike-on-halt) because a halt/refusal/log-check response names
/// the head to explain why it is *not* being done. A heading topic that merely
/// contains the id with trailing modifiers — `#id halt`, `#id deferred` — never
/// counts, for either head shape.
///
/// Two head shapes are id-backed directives, never heading-consumable:
///  1. A bare `do [#id]` / `do #id` directive (`queue_head_is_bare_do_directive`).
///  2. An operator-pinned bare id head (`[#id]` / `#id`, with or without a
///     priority pin) that resolves to exactly its own id AND whose id names a
///     tracked `agent:backlog` / `agent:review` item (#zwn5). Such a head is a
///     directive referencing a tracked item — e.g. an operator-drive live-verify
///     item the agent answers with a log-check but can never close itself — so a
///     `### Re: #id` heading must leave it pinned for an explicit
///     `--done`/`--pending-gate`/`--pending-edit` close. A registered prompt
///     preset id (e.g. `#spec-...`) is NOT a tracked item, so it stays synthetic
///     and still consumes on a matching heading.
pub(crate) fn response_targets_synthetic_queue_head_id(
    file: &Path,
    response: &str,
) -> Result<bool> {
    let content =
        std::fs::read_to_string(file).context("queue consume guard: failed to read document")?;
    let Some(queue_head) = active_queue_head_text(&content)? else {
        return Ok(false);
    };
    if queue_head_is_bare_do_directive(&queue_head) {
        return Ok(false);
    }
    let Some(head_id) = queue_prompt_done_id(&queue_head) else {
        return Ok(false);
    };
    // #zwn5: an operator-pinned bare id head that resolves to exactly its own id
    // and names a tracked backlog/review item is an id-backed directive, not a
    // synthetic/preset prompt. Leave it pinned for an explicit closeout flag
    // instead of striking it on a log-check/halt `### Re: #id` heading.
    let normalized_head = normalize_queue_prompt_text(&queue_head);
    if topic_resolves_to_exact_id(&normalized_head, &head_id)
        && head_id_names_tracked_directive_item(&content, &head_id)
    {
        return Ok(false);
    }
    Ok(response
        .lines()
        .filter_map(response_heading_topic)
        .any(|topic| topic_resolves_to_exact_id(topic, &head_id)))
}

/// Node keys of every non-struck free-text queue head that this cycle answered,
/// at ANY position in the queue (#ftstrike). Two signals mark a head answered:
///
/// 1. **Drain-target marker (`#qheadstrikeauto`).** The free-text head carrying
///    the in-progress `🚧` marker IS the head the binary dispatched this cycle, so
///    a committed (non-empty) response answers it by definition — struck without
///    requiring the agent to quote it as a `> **Queue prompt:**` blockquote
///    (operator: "the binary should do this automatically...not the agent").
/// 2. **Prose/blockquote answer-match (`#ftstrike`).** A non-marker free-text head
///    is struck when `response_body` quotes it, mirroring how
///    `mark_entries_completed_by_done_ids` marks id-backed heads regardless of
///    position, but keyed to the answering response instead of a tracked id.
///
/// `baseline` is the stable pre-turn document (the preflight baseline). When
/// supplied, a candidate head is struck only if it was already present in that
/// baseline (`#qstrikeexplain` Phase 2 — conservative strike): a head that first
/// appeared in the live buffer this turn defers to the cycle that actually
/// answers it. `None` skips the gate (legacy behavior / no baseline available).
pub(crate) fn answered_free_text_head_node_keys(
    content: &str,
    response_body: &str,
    baseline: Option<&str>,
) -> Result<Vec<String>> {
    if response_body.trim().is_empty() {
        return Ok(Vec::new());
    }
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue").map_err(|err| {
        anyhow::anyhow!("free-text strike: failed to derive queue node keys: {err}")
    })?;
    let mut keys = Vec::new();
    for node in nodes {
        if node.item.struck {
            continue;
        }
        let text = node.item.text.trim();
        if text.is_empty() || !queue_prompt_text_is_free_text(content, text) {
            continue;
        }
        // `#qheadstrikeauto`: the binary stamps the cycle's drain target with the
        // in-progress `🚧` marker at preflight (`set_first_prompt_in_progress`). A
        // free-text head carrying that marker IS the head this cycle was dispatched
        // to drain — so on a committed (non-empty) response it is answered by
        // definition, regardless of whether the agent quoted it as a
        // `> **Queue prompt:**` blockquote. Operator: "the binary should do this
        // automatically...not the agent." The prose/blockquote answer-match remains
        // as the secondary signal that strikes non-marker free-text heads answered
        // at any position (`#ftstrike`). The baseline gate below still applies, so a
        // marker on an in-flight operator edit (head absent from the pre-turn
        // baseline) is never struck.
        let is_drain_target_marker_head = head_carries_in_progress_marker(text);
        if !is_drain_target_marker_head && !free_text_head_answered_by_response(response_body, text)
        {
            continue;
        }
        // `#qstrikeexplain` Phase 2 — never strike a head still being authored this
        // turn. A free-text head that is NOT in the stable pre-turn baseline first
        // appeared in the live buffer during this turn (an in-flight operator edit);
        // defer it to the cycle that actually answers it (editor-wins, consistent
        // with #queue-user-edit-overwrite) instead of striking the line the operator
        // is typing.
        if let Some(baseline) = baseline
            && !free_text_head_present_in_baseline(baseline, text)
        {
            eprintln!(
                "[queue] #qstrikeexplain: deferring free-text head strike — head not in pre-turn baseline (in-flight operator edit)"
            );
            continue;
        }
        keys.push(node.node_key);
    }
    Ok(keys)
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
) -> Result<usize> {
    if response_body.trim().is_empty() {
        return Ok(0);
    }
    let _lock = acquire_doc_lock(file)?;
    let content =
        std::fs::read_to_string(file).context("free-text strike: failed to read document")?;
    let (fm, _) = frontmatter::parse(&content)?;
    if fm.queue_active != Some(true) {
        return Ok(0);
    }
    // `#qstrikeexplain` Phase 2: the stable pre-turn baseline gates which heads may
    // be struck — a head absent from it is an in-flight operator edit and must not
    // be struck this cycle. A missing baseline (rare; preflight writes it each
    // cycle) skips the gate so legacy strike behavior is preserved.
    let baseline = match snapshot::baseline_path_for(file) {
        Ok(path) => std::fs::read_to_string(&path).ok(),
        Err(_) => None,
    };
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
    let new_snapshot = match snapshot::load(file)? {
        Some(snap) => {
            let snap_keys =
                answered_free_text_head_node_keys(&snap, response_body, baseline.as_deref())?;
            if snap_keys.is_empty() {
                None
            } else {
                let snap_struck = consume_queue_nodes_by_key(&snap, &snap_keys)?;
                Some(annotate_newly_struck_free_text_heads(&snap, &snap_struck)?)
            }
        }
        None => None,
    };

    if skip_visible_guard {
        atomic_write(file, &new_document).context("free-text strike: failed to write document")?;
    } else {
        converge_document_or_disk(file, &new_document, &content, "free_text_strike")
            .context("free-text strike: failed to write document")?;
    }
    if let Some(snap) = new_snapshot {
        snapshot::save(file, &snap)?;
    }

    eprintln!(
        "[queue] struck {} answered free-text head(s) by response match (#ftstrike)",
        keys.len()
    );
    crate::ops_log::log_op(
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
            crate::ops_log::log_op(
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

/// Node keys of every active (non-struck) queue head that is non-drainable
/// **noise** (`#goqstall2` / `#qcontam`): pasted console output, an agent-response
/// fragment, or another structural/log artifact. `preset_supplies_directive` is
/// taken from the queue's `preset` attribute so classification matches
/// `queue_continuation::queue_stale_noise_lines` exactly. Id-backed directive heads
/// (`do [#id]`) and genuinely drainable free-text/prose heads are excluded, so
/// pruning never desyncs tracked or runnable work.
fn noise_queue_head_node_keys(content: &str) -> Result<Vec<String>> {
    let preset_supplies_directive = element::parse(content)
        .ok()
        .and_then(|comps| {
            comps
                .iter()
                .find(|c| c.name == "queue")
                .map(|c| c.attrs.contains_key("preset"))
        })
        .unwrap_or(false);
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
        .map_err(|err| anyhow::anyhow!("noise prune: failed to derive queue node keys: {err}"))?;
    let mut keys = Vec::new();
    for node in nodes {
        if node.item.struck {
            continue;
        }
        let text = node.item.text.trim();
        if text.is_empty() {
            continue;
        }
        if agent_doc_queue::queue_continuation::is_noise_queue_head(text, preset_supplies_directive)
        {
            keys.push(node.node_key);
        }
    }
    Ok(keys)
}

/// Node keys of every live (non-struck) **orphan id-backed** queue head: an
/// id-backed directive (`do [#id]` / `[#id]` / `#id`) whose id names NO open
/// `agent:backlog` item (#orphanqhead bulk prune / #qchurn). Such a head has no
/// drain path — `queue consume` rejects id-backed heads ("reap via --done") and
/// `--done <id>` is a no-op — yet it is excluded from `drainable_head_count` and,
/// when it sits at the queue head, BLOCKS the leading-run `queue consume` from
/// reaching answered free-text heads behind it, so the go-mode loop churns. Bulk
/// pruning it (alongside noise) clears that wedge without the operator naming each
/// id via the targeted `queue consume --id <id>` escape hatch.
///
/// Gated on an `agent:backlog` component being PRESENT: a free-form id-head queue
/// (no backlog) treats the id-heads AS the work, so membership is not required and
/// nothing is pruned — mirroring `head_is_drainable`'s `open_backlog_ids` gate so
/// the prune set and the drainable set agree on what an "orphan" is.
fn orphan_id_queue_head_node_keys(content: &str) -> Result<Vec<String>> {
    let has_backlog = element::parse(content)
        .map(|comps| comps.iter().any(|c| element::is_backlog_component(&c.name)))
        .unwrap_or(false);
    if !has_backlog {
        return Ok(Vec::new());
    }
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
        .map_err(|err| anyhow::anyhow!("orphan prune: failed to derive queue node keys: {err}"))?;
    let mut keys = Vec::new();
    for node in nodes {
        if node.item.struck {
            continue;
        }
        let text = node.item.text.trim();
        // Only id-backed heads are candidates; a free-text report that merely
        // contains a stray `#token` must never be force-struck here.
        if text.is_empty() || queue_prompt_text_is_free_text(content, text) {
            continue;
        }
        let Some(id) = queue_prompt_done_id(text) else {
            continue;
        };
        // An id naming OPEN backlog work (including a deferred `[operator-verify]` /
        // `[focused-cycle]` item, which is still an open `[ ]`/`[/]` entry) has a
        // real drain path — preserve it. Only a truly absent id is an orphan.
        if !head_id_names_open_backlog_item(content, &id) {
            keys.push(node.node_key);
        }
    }
    Ok(keys)
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
pub fn prune_noise_queue_heads(file: &Path) -> Result<usize> {
    let _lock = acquire_doc_lock(file)?;
    let content = std::fs::read_to_string(file).context("noise prune: failed to read document")?;
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
    let new_snapshot = match snapshot::load(file)? {
        Some(snap) => {
            let (new_snap, _) = strike_all_noise_queue_heads(&snap)?;
            if new_snap == snap {
                None
            } else {
                Some(new_snap)
            }
        }
        None => None,
    };

    converge_document_or_disk(file, &new_document, &content, "noise_prune")
        .context("noise prune: failed to write document")?;
    if let Some(snap) = new_snapshot {
        snapshot::save(file, &snap)?;
    }

    let base_hash = agent_doc_hash::content_hash(&content);
    eprintln!(
        "[queue] pruned {struck} predicate-proven head(s): noise + orphan id-backed (#goqstall2/#orphanqhead)"
    );
    crate::ops_log::log_op(
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

/// Clear every non-drainable queue head from `content`, returning the rewritten
/// document and the number struck. Two non-drainable classes are cleared: **noise**
/// (pasted console output / agent fragments / structural artifacts) and **orphan
/// id-backed heads** (`#orphanqhead`: a `do [#id]` / `[#id]` head whose id names no
/// open `agent:backlog` item). Multiline fenced noise blocks are excised by byte
/// range (`queue::parse_spans`); bulleted single-line noise AND orphan id heads are
/// struck by durable node key (`item_nodes`). Multiline removal runs first so the
/// node-key pass sees stable post-excision offsets. (#qnoise-multiline-strike)
fn strike_all_noise_queue_heads(content: &str) -> Result<(String, usize)> {
    let comps = element::parse(content)?;
    let Some(queue) = comps.iter().find(|c| c.name == "queue") else {
        return Ok((content.to_string(), 0));
    };
    let preset_supplies_directive = queue.attrs.contains_key("preset");
    let body_start = queue.open_end;
    let body = &content[body_start..queue.close_start];

    // 1. Multiline noise Prompt blocks AND pasted-evidence `Freeform` lines, by exact
    //    byte range (#qnoise-multiline-strike). A multiline `---`/~~~-fenced Prompt is
    //    excised only when its text is noise (multi-line console dump, nested ``` fence,
    //    agent-marker, or bold-report) — a single-line `do [#id]` directive that merely
    //    happens to be `---`-wrapped stays drainable and is preserved. A bare ```` ``` ````
    //    console paste (the most common operator flood) is not a recognized queue fence,
    //    so it lands as a run of `Freeform` lines instead; `is_noise_freeform_line`
    //    excises those while preserving `---`/`~~~` separators and `re [#id]` references.
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    for (entry, range) in agent_doc_queue::document_queue::parse_spans(body)? {
        let is_noise = match &entry {
            agent_doc_queue::document_queue::QueueEntry::Prompt(prompt) => {
                prompt.multiline
                    && agent_doc_queue::queue_continuation::is_noise_queue_head(
                        &prompt.text,
                        preset_supplies_directive,
                    )
            }
            agent_doc_queue::document_queue::QueueEntry::Freeform(line) => {
                agent_doc_queue::document_queue::is_noise_freeform_line(line)
            }
            _ => false,
        };
        if is_noise {
            ranges.push((body_start + range.start)..(body_start + range.end));
        }
    }
    let multiline_struck = ranges.len();
    let mut working = content.to_string();
    ranges.sort_by_key(|r| r.start);
    // Excise back-to-front so earlier offsets stay valid.
    for range in ranges.into_iter().rev() {
        working.replace_range(range, "");
    }

    // 2. Bulleted single-line noise heads AND orphan id-backed heads (#orphanqhead),
    //    struck via durable node keys. Orphan id-heads are non-drainable like noise
    //    but `is_noise_queue_head` keeps them (they carry an `#id`), so they are
    //    collected separately and merged into one strike set. Dedup so a head that
    //    somehow matches both passes is not double-counted.
    let mut keys = noise_queue_head_node_keys(&working)?;
    for key in orphan_id_queue_head_node_keys(&working)? {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    let bulleted_struck = keys.len();
    if !keys.is_empty() {
        working = consume_queue_nodes_by_key(&working, &keys)?;
    }

    Ok((working, multiline_struck + bulleted_struck))
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
pub fn strike_orphan_id_backed_queue_head(file: &Path, id: &str) -> Result<bool> {
    let _lock = acquire_doc_lock(file)?;
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("orphan strike: failed to read {}", file.display()))?;
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
    let new_snapshot = match snapshot::load(file)? {
        Some(snap) => {
            let snap_keys = id_backed_head_node_keys(&snap, &target_id)?;
            if snap_keys.is_empty() {
                None
            } else {
                Some(consume_queue_nodes_by_key(&snap, &snap_keys)?)
            }
        }
        None => None,
    };
    let base_hash = agent_doc_hash::content_hash(&content);
    converge_document_or_disk(file, &new_document, &content, "orphan_id_head_strike")
        .context("orphan strike: failed to write document")?;
    if let Some(snap) = new_snapshot {
        snapshot::save(file, &snap)?;
    }
    eprintln!(
        "[queue] struck orphaned id-backed head [#{target_id}] ({} node(s); #orphanqhead)",
        keys.len()
    );
    crate::ops_log::log_op(
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
pub fn acknowledge_open_id_backed_queue_head(file: &Path, id: &str) -> Result<bool> {
    let _lock = acquire_doc_lock(file)?;
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("open-id ack: failed to read {}", file.display()))?;
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
    let new_snapshot = match snapshot::load(file)? {
        Some(snap) => {
            let snap_keys = id_backed_head_node_keys(&snap, &target_id)?;
            if snap_keys.is_empty() {
                None
            } else {
                Some(consume_queue_nodes_by_key(&snap, &snap_keys)?)
            }
        }
        None => None,
    };
    let base_hash = agent_doc_hash::content_hash(&content);
    converge_document_or_disk(file, &new_document, &content, "open_id_head_ack")
        .context("open-id ack: failed to write document")?;
    if let Some(snap) = new_snapshot {
        snapshot::save(file, &snap)?;
    }
    eprintln!(
        "[queue] acknowledged id-backed correction head [#{target_id}] ({} node(s); backlog left open; #freshqueueauth)",
        keys.len()
    );
    crate::ops_log::log_op(
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

/// Node keys of every live (non-struck) id-backed queue head whose directive id
/// resolves to exactly `target_id`. Free-text prompts — even ones that merely
/// *contain* a `#token` — are excluded so the orphan escape hatch can never strike
/// a free-text operator report by accident.
fn id_backed_head_node_keys(content: &str, target_id: &str) -> Result<Vec<String>> {
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
        .map_err(|err| anyhow::anyhow!("orphan strike: failed to derive queue node keys: {err}"))?;
    let mut keys = Vec::new();
    for node in nodes {
        if node.item.struck {
            continue;
        }
        let text = node.item.text.trim();
        if text.is_empty() || queue_prompt_text_is_free_text(content, text) {
            continue;
        }
        if queue_prompt_done_id(text).as_deref() == Some(target_id) {
            keys.push(node.node_key);
        }
    }
    Ok(keys)
}

/// True when `target_id` names a NON-done item in any `agent:backlog` component —
/// live work that should drain through the normal `--done` lifecycle rather than
/// the orphan escape hatch.
fn head_id_names_open_backlog_item(content: &str, target_id: &str) -> bool {
    let Ok(comps) = agent_doc_element::element::parse(content) else {
        return false;
    };
    comps
        .iter()
        .filter(|c| agent_doc_element::element::is_backlog_component(&c.name))
        .any(|comp| {
            let (_, items, _) =
                agent_doc_element_backlog::backlog::parse_items(comp.content(content));
            items.iter().any(|item| {
                !item.is_done() && !item.id.is_empty() && item.id.eq_ignore_ascii_case(target_id)
            })
        })
}

/// Resolve whether this cycle's committed response should consume (strike) the
/// active queue head. Single source of truth for the strict-closeout decision so
/// successful closeouts advance the queue identically and never leave an answered
/// head queued to treadmill the auto-loop on the next preflight. Unproven IPC
/// attempts fail before queue consumption and must be retried.
///
/// Mirrors the layered signals: explicit `do queue` / prompt-target / `--done`
/// triggers, explicit `--done`/`--pending-gate`/`--review-resolve`/
/// `--pending-edit` completion of an id-backed head, a response heading that
/// resolves to a synthetic/preset head id, and a free-text head answered by this
/// cycle's response (unless the cycle answered a foreign `agent:exchange` prompt
/// instead).
pub(crate) fn queue_consumption_allowed_for_response(
    file: &Path,
    baseline: Option<&str>,
    current_content: &str,
    response_body: &str,
    completion_ids: &[String],
) -> Result<bool> {
    if should_consume_queue_prompt_for_write(file, baseline, current_content, completion_ids)? {
        return Ok(true);
    }
    if queue_head_has_explicit_completion_signal(current_content, completion_ids)? {
        return Ok(true);
    }
    let has_response = !response_body.trim().is_empty();
    if has_response && response_targets_synthetic_queue_head_id(file, response_body)? {
        return Ok(true);
    }
    if has_response
        && queue_head_is_free_text_prompt(current_content)?
        && let Some(head_text) = active_queue_head_text(current_content)?
    {
        return Ok(
            free_text_head_answered_by_response(response_body, &head_text)
                && !cycle_answered_foreign_exchange_prompt(baseline, current_content, &head_text),
        );
    }
    Ok(false)
}

pub(crate) fn mark_completed_queue_prompts_for_done_ids(
    file: &Path,
    done_ids: &[String],
    skip_visible_guard: bool,
) -> Result<usize> {
    if done_ids.is_empty() {
        return Ok(0);
    }

    let _lock = acquire_doc_lock(file)?;
    let content =
        std::fs::read_to_string(file).context("queue done-id mark: failed to read document")?;
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

    let new_snapshot = if let Some(snapshot_content) = snapshot::load(file)? {
        let snapshot_components = element::parse(&snapshot_content)?;
        let snapshot_queue = snapshot_components
            .iter()
            .find(|component| component.name == "queue")
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "queue done-id mark: document queue changed but snapshot has no agent:queue component"
                )
            })?;
        let snapshot_body = &snapshot_content[snapshot_queue.open_end..snapshot_queue.close_start];
        let snapshot_entries = agent_doc_queue::document_queue::parse(snapshot_body)
            .context("queue done-id mark: failed to parse snapshot queue")?;
        let (snapshot_marked_entries, snapshot_marked_texts) =
            mark_entries_completed_by_done_ids(&snapshot_entries, done_ids);
        if snapshot_marked_texts.len() != marked_texts.len() {
            anyhow::bail!(
                "queue done-id mark: snapshot matched {} queue item(s) but document matched {}",
                snapshot_marked_texts.len(),
                marked_texts.len()
            );
        }
        let snapshot_body = agent_doc_queue::document_queue::render(&snapshot_marked_entries);
        Some(snapshot_queue.replace_content(&snapshot_content, &snapshot_body))
    } else {
        None
    };

    // `#fcc0`: converge the done-id mark write through the editor IPC when a JB
    // listener is active (no `File Cache Conflict` dialog); fall back to the
    // guarded disk write otherwise. The force-disk repair path keeps its raw
    // bypass — it deliberately skips IPC/IDE and the visible-write guard.
    if skip_visible_guard {
        atomic_write(file, &new_document)
            .context("queue done-id mark: failed to write document")?;
    } else {
        converge_document_or_disk(file, &new_document, &content, "queue_done_id_mark")
            .context("queue done-id mark: failed to write document")?;
    }
    if let Some(new_snapshot) = new_snapshot {
        snapshot::save(file, &new_snapshot)?;
    }

    eprintln!(
        "[queue] marked {} completed item(s) by done id: {:?}",
        marked_texts.len(),
        marked_texts
    );
    Ok(marked_texts.len())
}

pub(crate) struct QueuePromptNodeKeys {
    keys: Vec<String>,
    ast_backed: bool,
}

fn queue_prompt_node_keys_for_texts(
    content: &str,
    target_texts: &[String],
    preferred_node_keys: &[String],
) -> Result<Option<QueuePromptNodeKeys>> {
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
        .map_err(|err| anyhow::anyhow!("queue consume: failed to derive queue node keys: {err}"))?;
    let mut selected_indices = std::collections::HashSet::new();
    let mut keys = Vec::with_capacity(target_texts.len());
    for (target_index, target_text) in target_texts.iter().enumerate() {
        let preferred = preferred_node_keys.get(target_index);
        let preferred_index = preferred.and_then(|preferred_key| {
            nodes.iter().enumerate().position(|(node_index, node)| {
                !selected_indices.contains(&node_index)
                    && !node.item.struck
                    && node.node_key == *preferred_key
                    && queue_prompt_texts_match_for_consumption(&node.item.text, target_text)
            })
        });
        let fallback_index = || {
            nodes.iter().enumerate().position(|(node_index, node)| {
                !selected_indices.contains(&node_index)
                    && !node.item.struck
                    && queue_prompt_texts_match_for_consumption(&node.item.text, target_text)
            })
        };
        let Some(node_index) = preferred_index.or_else(fallback_index) else {
            return Ok(None);
        };
        selected_indices.insert(node_index);
        keys.push(nodes[node_index].node_key.clone());
    }
    Ok(Some(QueuePromptNodeKeys {
        keys,
        ast_backed: true,
    }))
}

pub(crate) fn queue_prompt_node_keys_for_count(
    content: &str,
    count: usize,
) -> Result<QueuePromptNodeKeys> {
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
        .map_err(|err| anyhow::anyhow!("queue consume: failed to derive queue node keys: {err}"))?;
    let ast_keys = nodes
        .into_iter()
        .filter(|node| !node.item.struck)
        .take(count)
        .map(|node| node.node_key)
        .collect::<Vec<_>>();
    if ast_keys.len() >= count {
        return Ok(QueuePromptNodeKeys {
            keys: ast_keys,
            ast_backed: true,
        });
    }

    let components = element::parse(content)?;
    let queue_component = components
        .iter()
        .find(|c| c.name == "queue")
        .ok_or_else(|| anyhow::anyhow!("queue consume: document has no agent:queue component"))?;
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = agent_doc_queue::document_queue::parse(body)
        .context("queue consume: failed to parse document queue")?;
    let prompt_texts = first_n_queue_prompt_texts(&entries, count);
    if prompt_texts.len() < count {
        anyhow::bail!(
            "queue consume: document has {} prompt(s) but planned to consume {}",
            prompt_texts.len(),
            count
        );
    }

    let keys = prompt_texts
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let hash = agent_doc_hash::content_hash(text);
            let short_hash = &hash[..hash.len().min(12)];
            format!("queue:entry:{index}:{short_hash}")
        })
        .collect::<Vec<_>>();

    Ok(QueuePromptNodeKeys {
        keys,
        ast_backed: false,
    })
}

pub(crate) fn queue_prompt_node_keys_for_done_ids(
    content: &str,
    done_ids: &[String],
    consumed_texts: &[String],
) -> QueuePromptNodeKeys {
    let done_ids = done_ids
        .iter()
        .map(|id| normalize_done_id(id))
        .collect::<std::collections::HashSet<_>>();

    if let Ok(nodes) = agent_doc_markdown_ast::mutations::item_nodes(content, "queue") {
        let keys = nodes
            .into_iter()
            .filter(|node| !node.item.struck)
            .filter(|node| {
                queue_prompt_done_id(&node.item.text).is_some_and(|id| done_ids.contains(&id))
            })
            .map(|node| node.node_key)
            .collect::<Vec<_>>();
        if keys.len() == consumed_texts.len() {
            return QueuePromptNodeKeys {
                keys,
                ast_backed: true,
            };
        }
    }

    let keys = consumed_texts
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let hash = agent_doc_hash::content_hash(text);
            let short_hash = &hash[..hash.len().min(12)];
            format!("queue:done:{index}:{short_hash}")
        })
        .collect::<Vec<_>>();
    QueuePromptNodeKeys {
        keys,
        ast_backed: false,
    }
}

pub(crate) fn consume_queue_nodes_by_key(content: &str, node_keys: &[String]) -> Result<String> {
    let borrowed = node_keys.iter().map(String::as_str).collect::<Vec<_>>();
    let consumed = agent_doc_markdown_ast::mutations::consume_nodes(content, "queue", &borrowed)
        .map_err(|err| {
            anyhow::anyhow!("queue consume: failed to apply node-keyed consume: {err}")
        })?;
    Ok(strip_in_progress_marker_from_struck_queue_items(&consumed))
}

fn strip_in_progress_marker_from_struck_queue_items(content: &str) -> String {
    let Ok(components) = element::parse(content) else {
        return content.to_string();
    };
    let Some(queue) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return content.to_string();
    };
    let body = queue.content(content);
    let needle_with_space = format!("~~{} ", IN_PROGRESS_MARKER);
    let needle_bare = format!("~~{}", IN_PROGRESS_MARKER);
    let updated_body = body
        .replace(&needle_with_space, "~~")
        .replace(&needle_bare, "~~");
    if updated_body == body {
        content.to_string()
    } else {
        queue.replace_content(content, &updated_body)
    }
}

pub(crate) fn plan_queue_prompt_consumption(
    file: &Path,
    content: &str,
    done_ids: &[String],
) -> Result<Option<QueueConsumptionPlan>> {
    let snapshot_content = snapshot::load(file)?;
    plan_queue_prompt_consumption_with_snapshot(
        file,
        content,
        snapshot_content.as_deref(),
        done_ids,
    )
}

pub(crate) fn plan_queue_prompt_consumption_with_snapshot(
    file: &Path,
    content: &str,
    snapshot_content: Option<&str>,
    done_ids: &[String],
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
            let node_ops = consumed_node_keys
                .keys
                .iter()
                .cloned()
                .map(|node_key| IpcNodeOp::consume("queue", node_key))
                .collect::<Vec<_>>();

            let has_auto = agent_doc_queue::document_queue::has_auto_attr(&comp.attrs);
            let remaining = agent_doc_queue::document_queue::prompts(&completed_entries).len();
            let drained = remaining == 0;
            let new_entries = if drained {
                Vec::new()
            } else {
                completed_entries
            };
            let new_body = agent_doc_queue::document_queue::render(&new_entries);
            let mut current = if drained || !consumed_node_keys.ast_backed {
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

            let snap = snapshot_content.ok_or_else(|| {
                anyhow::anyhow!("queue consume: queue_active is true but snapshot is missing")
            })?;
            let snap_comps = element::parse(snap)?;
            let snap_queue = snap_comps
                .iter()
                .find(|c| c.name == "queue")
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "queue consume: queue_active is true but snapshot has no agent:queue component"
                    )
                })?;
            let snap_body = &snap[snap_queue.open_end..snap_queue.close_start];
            let snap_entries = agent_doc_queue::document_queue::parse(snap_body)
                .context("queue consume: failed to parse snapshot queue")?;
            let snap_has_auto = agent_doc_queue::document_queue::has_auto_attr(&snap_queue.attrs);
            let (snap_completed_entries, snapshot_consumed_texts) =
                mark_entries_completed_by_done_ids(&snap_entries, done_ids);
            if normalized_done_id_bag(&snapshot_consumed_texts)
                != normalized_done_id_bag(&consumed_texts)
            {
                anyhow::bail!(
                    "queue consume: snapshot done-id prompts {:?} do not match document done-id prompts {:?}",
                    snapshot_consumed_texts,
                    consumed_texts
                );
            }
            let snapshot_node_keys =
                queue_prompt_node_keys_for_done_ids(snap, done_ids, &snapshot_consumed_texts);
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
                crate::ops_log::log_op(
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

            let mut new_snap =
                if drained || snap_new_entries != new_entries || !snapshot_node_keys.ast_backed {
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

            let response_first_line = crate::capture::load_active(file)
                .ok()
                .flatten()
                .and_then(|c| first_nonempty_line(&c.response_body).map(str::to_string));
            current = embed_consumed_prompt_in_response(
                &current,
                &consumed_texts,
                response_first_line.as_deref(),
            );
            new_snap = embed_consumed_prompt_in_response(
                &new_snap,
                &consumed_texts,
                response_first_line.as_deref(),
            );
            let save_snapshot = new_snap != snap;

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

    let consume_count = leading_done_consume_count.max(1);
    let mut consumed_texts = first_n_queue_prompt_texts(&entries, consume_count);
    let mut consumed_text = consumed_texts.first().cloned().ok_or_else(|| {
        anyhow::anyhow!(
            "queue consume: queue_active is true but document queue has no prompt to consume"
        )
    })?;

    // Update snapshot in sync. Required closeouts must be able to prove the
    // same active turn prompt was removed from both the file and the snapshot.
    // Load the snapshot before mutating the document so live queue insertions can
    // be preserved while the pre-turn active head remains the closeout target.
    let snap = snapshot_content.ok_or_else(|| {
        anyhow::anyhow!("queue consume: queue_active is true but snapshot is missing")
    })?;
    let snap_comps = element::parse(snap)?;
    let snap_queue = snap_comps
        .iter()
        .find(|c| c.name == "queue")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "queue consume: queue_active is true but snapshot has no agent:queue component"
            )
        })?;
    let snap_body = &snap[snap_queue.open_end..snap_queue.close_start];
    let snap_entries = agent_doc_queue::document_queue::parse(snap_body)
        .context("queue consume: failed to parse snapshot queue")?;
    let snap_has_auto = agent_doc_queue::document_queue::has_auto_attr(&snap_queue.attrs);
    let snapshot_consumed_texts = first_n_queue_prompt_texts(&snap_entries, consume_count);
    let snapshot_node_keys = queue_prompt_node_keys_for_count(snap, consume_count)?;
    if snapshot_consumed_texts.len() != consumed_texts.len() {
        anyhow::bail!(
            "queue consume: snapshot has {} prompt(s) available but document consumed {}",
            snapshot_consumed_texts.len(),
            consumed_texts.len()
        );
    }
    // Compare head identity ignoring cosmetic pin annotations
    // (`#queue-consume-pushpin-normalization`): the snapshot can carry the
    // unpinned spelling of a head while the live document carries the `:pushpin:`
    // spelling of the same logical item. The pin is priority metadata, not
    // identity, so a raw text comparison spuriously fails the cycle. Normalize
    // both sides through `strip_priority_markers` before the equality check.
    let norm = |texts: &[String]| {
        texts
            .iter()
            .map(|text| strip_priority_markers(text))
            .collect::<Vec<_>>()
    };
    let mut consume_snapshot_head_for_live_addition = false;
    if norm(&snapshot_consumed_texts) != norm(&consumed_texts) {
        // A dropped-queue record proves a live editor-buffer addition exists and
        // must be preserved. It does NOT retarget the active turn. When the
        // document head is that recorded addition, consume the snapshot active
        // head wherever it now sits in the current queue and leave the insertion
        // live for a later turn.
        let dropped_evidence = crate::cycle_state::load(file)
            .ok()
            .flatten()
            .map(|s| s.dropped_queue_prompts)
            .unwrap_or_default();
        let doc_head_is_recorded_addition = !dropped_evidence.is_empty()
            && consumed_texts.iter().all(|doc_head| {
                let doc_norm = strip_priority_markers(doc_head);
                dropped_evidence
                    .iter()
                    .any(|d| strip_priority_markers(d) == doc_norm)
            });
        if doc_head_is_recorded_addition {
            consume_snapshot_head_for_live_addition = true;
        } else {
            anyhow::bail!(
                "queue consume: snapshot head prompts {:?} do not match document head prompts {:?}",
                snapshot_consumed_texts,
                consumed_texts
            );
        }
    }
    let (consumed_node_keys, completed_entries) = if consume_snapshot_head_for_live_addition {
        if snapshot_consumed_texts
            .iter()
            .any(|text| !queue_prompt_text_is_free_text(content, text))
        {
            crate::ops_log::log_op(
                file,
                &format!(
                    "queue_consume_refused_id_backed_snapshot_head_without_explicit_signal file={} head={:?} doc_head={:?}",
                    file.display(),
                    snapshot_consumed_texts,
                    consumed_texts
                ),
            );
            return Ok(None);
        }
        let Some(node_keys) = queue_prompt_node_keys_for_texts(
            content,
            &snapshot_consumed_texts,
            &snapshot_node_keys.keys,
        )?
        else {
            crate::ops_log::log_op(
                file,
                &format!(
                    "queue_consume_head_divergence_preserved_live_addition file={} reason=snapshot_active_head_missing snap_head={:?} doc_head={:?}",
                    file.display(),
                    snapshot_consumed_texts,
                    consumed_texts
                ),
            );
            return Ok(None);
        };
        let Some(completed_entries) =
            mark_first_matching_prompts_completed_by_texts(&entries, &snapshot_consumed_texts)
        else {
            crate::ops_log::log_op(
                file,
                &format!(
                    "queue_consume_head_divergence_preserved_live_addition file={} reason=snapshot_active_head_unrenderable snap_head={:?} doc_head={:?}",
                    file.display(),
                    snapshot_consumed_texts,
                    consumed_texts
                ),
            );
            return Ok(None);
        };
        crate::ops_log::log_op(
            file,
            &format!(
                "queue_consume_head_divergence_reconciled file={} reason=snapshot_active_head_authoritative_preserved_live_addition consumed={} snap_head={:?} doc_head={:?}",
                file.display(),
                consume_count,
                snapshot_consumed_texts,
                consumed_texts
            ),
        );
        consumed_texts = snapshot_consumed_texts.clone();
        consumed_text = consumed_texts.first().cloned().ok_or_else(|| {
            anyhow::anyhow!(
                "queue consume: snapshot-selected consume path had no prompt to consume"
            )
        })?;
        (node_keys, completed_entries)
    } else {
        if leading_done_consume_count == 0 && !queue_head_is_free_text_prompt(content)? {
            crate::ops_log::log_op(
                file,
                &format!(
                    "queue_consume_refused_id_backed_head_without_explicit_signal file={} head={:?}",
                    file.display(),
                    consumed_text
                ),
            );
            return Ok(None);
        }
        (
            queue_prompt_node_keys_for_count(content, consume_count)?,
            agent_doc_queue::document_queue::mark_first_n_prompts_completed(
                &entries,
                consume_count,
            ),
        )
    };
    let node_ops = consumed_node_keys
        .keys
        .iter()
        .cloned()
        .map(|node_key| IpcNodeOp::consume("queue", node_key))
        .collect::<Vec<_>>();

    let has_auto = agent_doc_queue::document_queue::has_auto_attr(&comp.attrs);
    let remaining = agent_doc_queue::document_queue::prompts(&completed_entries).len();
    let drained = remaining == 0;
    let new_entries = if drained {
        Vec::new()
    } else {
        completed_entries
    };
    let new_body = agent_doc_queue::document_queue::render(&new_entries);
    let mut current = if drained || !consumed_node_keys.ast_backed {
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
    let snap_completed_entries = agent_doc_queue::document_queue::mark_first_n_prompts_completed(
        &snap_entries,
        consume_count,
    );
    let snap_remaining = agent_doc_queue::document_queue::prompts(&snap_completed_entries).len();
    let snap_new_entries = if snap_remaining == 0 {
        Vec::new()
    } else {
        snap_completed_entries
    };
    if snap_new_entries != new_entries {
        // #finalize-divergence-orphans-committed-head / IPC-CRDT resilience: the
        // document `content` here is the post-CRDT-merge result — the merge has
        // already reconciled the agent (snapshot) side against concurrent
        // user/editor edits on the disk side. The same-head proof above
        // (`snapshot_consumed_texts == consumed_texts`) already confirmed we
        // consumed the right head; this remaining-queue difference is exactly the
        // concurrent edit the CRDT merge resolved. Hard-bailing here re-rejected
        // the merge the pipeline just succeeded at, leaving an orphaned unstruck
        // head that re-serves (the divergence error hit repeatedly under live
        // editor races). Reconcile instead: the merged document queue is
        // authoritative, and the snapshot below adopts the document's `new_body`,
        // so both sides converge on the head-struck merged state. Record the
        // reconciliation for forensics rather than failing the cycle.
        let snap_remaining_prompts =
            agent_doc_queue::document_queue::prompts(&snap_new_entries).len();
        let doc_remaining_prompts = agent_doc_queue::document_queue::prompts(&new_entries).len();
        crate::ops_log::log_op(
            file,
            &format!(
                "queue_consume_divergence_reconciled file={} reason=crdt_merge_authoritative consumed={} snap_remaining={} doc_remaining={}",
                file.display(),
                consume_count,
                snap_remaining_prompts,
                doc_remaining_prompts
            ),
        );
    }

    let mut new_snap =
        if drained || snap_new_entries != new_entries || !snapshot_node_keys.ast_backed {
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

    // #queue-prompt-echo-in-response: an auto/synthetic queue head is never typed
    // into `agent:exchange`, so a consumed queue turn would otherwise record only
    // the `### Re:` answer with no trace of the originating prompt. Embed the
    // consumed prompt text into this cycle's response block (in BOTH the document
    // and the snapshot, so the selective-commit boundary stays consistent) when
    // the prompt is not already present in the exchange. Fail-safe: any locator
    // miss leaves the content unchanged rather than risk corrupting the exchange.
    let response_first_line = crate::capture::load_active(file)
        .ok()
        .flatten()
        .and_then(|c| first_nonempty_line(&c.response_body).map(str::to_string));
    current = embed_consumed_prompt_in_response(
        &current,
        &consumed_texts,
        response_first_line.as_deref(),
    );
    new_snap = embed_consumed_prompt_in_response(
        &new_snap,
        &consumed_texts,
        response_first_line.as_deref(),
    );

    if new_snap != snap {
        return Ok(Some(QueueConsumptionPlan {
            consumed_text,
            consumed_texts,
            node_ops,
            remaining,
            drained,
            auto: has_auto,
            new_document: current,
            new_snapshot: new_snap,
            save_snapshot: true,
        }));
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
        save_snapshot: false,
    }))
}

#[cfg(test)]
mod core_tests {
    #![allow(unused_imports)]
    use super::*;
    use fs2::FileExt;
    use std::fs;
    use std::fs::OpenOptions;
    use std::time::Duration;
    use tempfile::TempDir;

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
        snapshot::save(&doc, &content).unwrap();

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
        snapshot::save(&doc, content).unwrap();

        let marked =
            mark_completed_queue_prompts_for_done_ids(&doc, &["opportunistic".to_string()], true)
                .unwrap();
        assert_eq!(marked, 1);

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("- do [#head]\n"), "{updated}");
        assert!(updated.contains("- ~~do [#opportunistic]~~\n"), "{updated}");
        assert!(updated.contains("- do [#tail]\n"), "{updated}");
        let snapshot = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snapshot.contains("- ~~do [#opportunistic]~~\n"),
            "{snapshot}"
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
        assert!(!queue_head_is_free_text_prompt(crate::test_support::HALT_QUEUE_DOC).unwrap());
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
        snapshot::save(&doc, content).unwrap();

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
        snapshot::save(&doc, content).unwrap();

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
        let ledger_path = crate::flow::proof_ledger::proof_ledger_path(root, &canonical);
        let records = crate::flow::proof_ledger::read_operation_proofs(&ledger_path).unwrap();
        assert_eq!(records.len(), 2, "ledger records: {records:#?}");
        assert_eq!(
            records[0].operation_kind,
            crate::flow::proof_ledger::ProofOperationKind::QueueHead
        );
        assert_eq!(
            records[0].outcome,
            crate::flow::proof_ledger::ProofOutcome::Recorded
        );
        assert_eq!(
            records[0].proof_kind,
            crate::flow::proof_ledger::ProofEvidenceKind::QueueHeadIdentity
        );
        assert!(records[0].proof.contains("phase=before_mutation"));
        assert!(records[0].proof.contains("Run queued thing"));
        assert_eq!(
            records[0].content_hash,
            agent_doc_hash::content_hash("Run queued thing")
        );
        assert_eq!(
            records[1].operation_kind,
            crate::flow::proof_ledger::ProofOperationKind::QueueHead
        );
        assert_eq!(
            records[1].outcome,
            crate::flow::proof_ledger::ProofOutcome::Consumed
        );
        assert_eq!(
            records[1].proof_kind,
            crate::flow::proof_ledger::ProofEvidenceKind::WriteResult
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
        let state_ledger = crate::project_controller::load_state_event_ledger(root)
            .expect("queue state events should reload from sqlite");
        let queue_events = state_ledger
            .events()
            .iter()
            .filter(|event| {
                matches!(
                    &event.fact,
                    crate::state_backbone::StateFact::QueueHeadSelected { .. }
                        | crate::state_backbone::StateFact::QueueHeadCompleted { .. }
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(queue_events.len(), 2, "queue events: {queue_events:#?}");
        assert!(
            matches!(
                &queue_events[0].fact,
                crate::state_backbone::StateFact::QueueHeadSelected { node_key, .. }
                    if node_key == &node_id
            ),
            "first queue state event should select the consumed head: {queue_events:#?}"
        );
        assert!(
            matches!(
                &queue_events[1].fact,
                crate::state_backbone::StateFact::QueueHeadCompleted { node_key, .. }
                    if node_key == &node_id
            ),
            "second queue state event should complete the consumed head: {queue_events:#?}"
        );

        let document_hash = queue_state_document_hash(&canonical);
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
        assert_eq!(head.phase, crate::state_backbone::QueueHeadPhase::Completed);
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
        snapshot::save(&doc, content).unwrap();

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
        let document_hash = queue_state_document_hash(&doc.canonicalize().unwrap());
        let state_ledger = crate::project_controller::load_state_event_ledger(root)
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
        assert_eq!(next.phase, crate::state_backbone::QueueHeadPhase::Selected);
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
        snapshot::save(&doc, content).unwrap();

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
        let document_hash = queue_state_document_hash(&doc.canonicalize().unwrap());
        let state_ledger = crate::project_controller::load_state_event_ledger(root)
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
        assert_eq!(next.phase, crate::state_backbone::QueueHeadPhase::Deferred);
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
        snapshot::save(&doc, snap).unwrap();

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
        let snap_result = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap_result.contains("- user added later"),
            "snapshot must adopt the reconciled document queue:\n{snap_result}"
        );
    }
    #[test]
    fn queue_consume_head_divergence_preserves_dropped_queue_evidence() {
        // #editorbufwin / realtime-turn split: the snapshot head is the OLD turn
        // target; the document head is the user's live editor-buffer addition.
        // Dropped-queue evidence proves the live edit must be preserved, not that
        // it should replace the turn target. If the old target no longer exists in
        // the document queue, closeout must no-op rather than consuming/deleting
        // the operator's new queue item.
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
        snapshot::save(&doc, snap).unwrap();

        // Record the live-buffer drift evidence for the document head.
        crate::cycle_state::start_preflight(&doc, Some(snap), Some(content)).unwrap();
        crate::cycle_state::record_dropped_queue_prompts(
            &doc,
            &["handle the new live-buffer request".to_string()],
        )
        .unwrap();

        let outcome = consume_queue_prompt_force_disk(&doc)
            .expect("dropped-queue evidence must avoid a hard bail");
        assert!(
            outcome.is_none(),
            "missing snapshot head should not consume the live queue edit"
        );
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("- handle the new live-buffer request"),
            "the live queue item must stay queued:\n{result}"
        );
        let snap_result = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap_result.contains("- handle the old request"),
            "no-consume path leaves the snapshot unchanged for later reconciliation:\n{snap_result}"
        );
        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops_log.contains("queue_consume_head_divergence_preserved_live_addition")
                && ops_log.contains("snapshot_active_head_missing"),
            "the preserved-live-edit no-op must be logged for forensics:\n{ops_log}"
        );
    }

    #[test]
    fn queue_consume_preserves_inserted_live_head_and_consumes_snapshot_head() {
        // A live queue edit is realtime state, not a turn retarget. If the
        // operator inserts a new free-text head while a turn is closing out, the
        // inserted head must stay queued and the pre-turn snapshot head must be
        // the item consumed for this turn.
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
        snapshot::save(&doc, snap).unwrap();

        crate::cycle_state::start_preflight(&doc, Some(snap), Some(content)).unwrap();
        crate::cycle_state::record_dropped_queue_prompts(&doc, &["test".to_string()]).unwrap();

        let outcome = consume_queue_prompt_force_disk(&doc)
            .expect("consume must preserve the live edit and close the snapshot head")
            .expect("snapshot head should be consumed");
        assert_eq!(outcome.consumed_text, "handle the old request");

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("- test\n"),
            "newly typed queue item must stay live:\n{result}"
        );
        assert!(
            !result.contains("- ~~test~~"),
            "newly typed queue item must not be consumed by the old turn:\n{result}"
        );
        assert!(
            result.contains("- ~~handle the old request~~"),
            "snapshot head must be consumed in place:\n{result}"
        );
        let snap_result = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap_result.contains("- test\n")
                && snap_result.contains("- ~~handle the old request~~"),
            "snapshot must adopt the preserved live edit and consumed old head:\n{snap_result}"
        );
    }

    #[test]
    fn queue_consume_head_divergence_without_evidence_still_bails() {
        // #editorbufwin (Fix A) corruption guard: a head divergence with NO
        // recorded dropped-queue evidence is unexplained (genuine corruption) and
        // must keep the hard-bail.
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
        snapshot::save(&doc, snap).unwrap();
        // No cycle_state dropped-queue evidence recorded.

        let err = consume_queue_prompt_force_disk(&doc)
            .expect_err("an unexplained head divergence must still hard-bail");
        assert!(
            err.to_string()
                .contains("do not match document head prompts"),
            "the corruption guard must keep the original bail: {err}"
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
        snapshot::save(&doc, content).unwrap();

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
        let snap = snapshot::load(&doc).unwrap().unwrap();
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
        snapshot::save(&doc, content).unwrap();

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
        snapshot::save(&doc, content).unwrap();

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
            !free_text_head_answered_by_response(
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
    fn strike_answered_free_text_heads_strikes_behind_id_head_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#fullboundary]\n",
            "- :pushpin: JB `Run Agent Doc` is stalled on this document when I tried to start the queue run. No notification.\n",
            "- :pushpin: My free-text queue items are not immediately struck as if they are addressed.\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

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
        assert!(
            !after_again.contains("auto-struck")
                || !after_again.contains("<!-- agent:exchange -->"),
            "this fixture has no exchange; note must never target exchange:\n{after_again}"
        );
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
        snapshot::save(&doc, content).unwrap();

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
        let snap = snapshot::load(&doc).unwrap().unwrap();
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
        snapshot::save(&doc, content).unwrap();

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
        let snap = snapshot::load(&doc).unwrap().unwrap();
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
        snapshot::save(&doc, content).unwrap();

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
        let snap = snapshot::load(&doc).unwrap().expect("snapshot saved");
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
        snapshot::save(&doc, content).unwrap();

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
        snapshot::save(&doc, content).unwrap();

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
        snapshot::save(&doc, content).unwrap();

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
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(snap, planned, "detached disk prune should update snapshot");
    }
}
