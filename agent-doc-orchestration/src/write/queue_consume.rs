//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) struct QueueConsumptionPlan {
    consumed_text: String,
    consumed_texts: Vec<String>,
    node_ops: Vec<IpcNodeOp>,
    remaining: usize,
    drained: bool,
    auto: bool,
    new_document: String,
    new_snapshot: String,
    save_snapshot: bool,
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
    if let Err(err) = crate::recguard_wedge::clear(file) {
        eprintln!(
            "[recguard-wedge] WARNING: failed to clear wedge counter for {}: {}",
            file.display(),
            err
        );
    }

    Ok(Some(outcome))
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
    done_ids: &[String],
) -> Result<bool> {
    // An explicit `--done` naming the queue head authorizes consumption
    // regardless of any pending mutations bundled into the same diff
    // (#pending-add-suppresses-queue-consume). Check it FIRST so a bundled
    // `--pending-add` cannot make the diff-based check below emit a misleading
    // "active prompt differs from queue head" diagnostic for a turn that does
    // in fact complete the head.
    if queue_head_matches_done_ids(current_content, done_ids)? {
        return Ok(true);
    }
    let Some(base) = baseline else {
        return Ok(false);
    };
    let base_norm = crate::diff::strip_comments(&strip_boundary_for_dedup(base));
    let current_norm = crate::diff::strip_comments(&strip_boundary_for_dedup(current_content));
    let diff_text = crate::diff::unified_diff_from_contents(&base_norm, &current_norm);
    should_consume_queue_prompt_for_diff_content(file, current_content, diff_text.as_deref())
}

pub(crate) fn queue_skip_diagnostic_for_file(file: &Path) -> Result<String> {
    let content =
        std::fs::read_to_string(file).context("queue skip diagnostic: failed to read document")?;
    queue_skip_diagnostic_for_content(&content)
}

pub(crate) fn queue_skip_diagnostic_for_content(content: &str) -> Result<String> {
    const GENERIC: &str =
        "[queue] skipped consumption because the active prompt did not target the queue head";

    let Some(queue_head) = active_queue_head_text(content)? else {
        return Ok(GENERIC.to_string());
    };
    let queue_head_display = display_queue_prompt_text(&queue_head);
    if queue_head_is_free_text_prompt(content)? {
        return Ok(format!(
            "[queue] kept free-text head `{queue_head_display}` because free-text heads are consumed by an answering `### Re:` response, not a tracked id outcome. Confirm the response targets this head (heading/topic match) so the answered-response path can strike it; otherwise it stays queued."
        ));
    }
    if let Some(id) = queue_prompt_done_id(&queue_head) {
        return Ok(format!(
            "[queue] kept head `{queue_head_display}` because the response did not record a completion outcome for #{id}. Reap it with `--done {id}`, gate it with `--pending-gate {id}`, or keep/narrow it with `--pending-edit \"{id}=...\"`. (missing proof: no done/gate/reap recorded for #{id} this cycle)"
        ));
    }
    Ok(GENERIC.to_string())
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
    let prompt_changes: Vec<_> = crate::diff::classify_prompt_bearing_changes(diff_text)
        .into_iter()
        .filter(|change| {
            matches!(
                change.kind,
                crate::diff::PromptBearingChangeKind::PromptTarget
                    | crate::diff::PromptBearingChangeKind::ContentEdit
            )
        })
        .collect();
    if crate::diff::detect_queue_trigger(diff_text) {
        return Ok(true);
    }
    if prompt_changes
        .iter()
        .any(|change| queue_prompt_text_matches(&change.text, &queue_head))
    {
        return Ok(true);
    }

    // Not a user-facing failure on its own: the caller still has explicit
    // completion-signal fallbacks (`--done`/`--pending-gate`/`--pending-edit`,
    // synthetic-head heading match). Only the caller's final "skipped
    // consumption" line is the authoritative skip signal, so record this detail
    // to ops_log instead of stderr to avoid a false-alarm during a turn that
    // ultimately consumes the head (#pending-add-suppresses-queue-consume).
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
    let base_norm = crate::diff::strip_comments(&strip_boundary_for_dedup(base));
    let current_norm = crate::diff::strip_comments(&strip_boundary_for_dedup(current_content));
    let Some(diff_text) = crate::diff::unified_diff_from_contents(&base_norm, &current_norm) else {
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

pub(crate) fn active_queue_head_text(content: &str) -> Result<Option<String>> {
    let (fm, _) = frontmatter::parse(content)?;
    if fm.queue_active != Some(true) {
        return Ok(None);
    }
    let components = component::parse(content)?;
    let comp = components
        .iter()
        .find(|component| component.name == "queue")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "queue consume guard: queue_active is true but document has no agent:queue component"
            )
        })?;
    let body = &content[comp.open_end..comp.close_start];
    let entries =
        crate::queue::parse(body).context("queue consume guard: failed to parse document queue")?;
    Ok(crate::queue::first_prompt(&entries).map(|prompt| prompt.text.clone()))
}

/// True when a closeout flag in this cycle explicitly names the active queue
/// head's `#id` — `--done`, `--pending-gate`, or `--pending-edit "<id>=…"`.
///
/// This is the explicit completion signal that authorizes queue-head consumption
/// (#queue-strike-on-halt). A `### Re:` heading that merely mentions the head id
/// is not a completion signal — a halt/refusal response names the head to explain
/// why it is *not* being completed — so consumption is driven by an explicit
/// closeout flag, never by heading text. `--pending-edit` counts because the
/// agent rewrote the item's tracked text as part of resolving it.
pub(crate) fn queue_head_has_explicit_completion_signal(
    content: &str,
    pending_done: &[String],
    pending_gate: &[String],
    pending_edit: &[String],
) -> Result<bool> {
    let Some(queue_head) = active_queue_head_text(content)? else {
        return Ok(false);
    };
    let Some(head_id) = queue_prompt_done_id(&queue_head) else {
        return Ok(false);
    };
    // `--done`/`--pending-gate` entries are bare ids; `--pending-edit` entries are
    // `"<id>=new text"`, so take the id segment before `=`.
    let names_head = |raw: &str| {
        let id = raw.split_once('=').map(|(id, _)| id).unwrap_or(raw);
        normalize_done_id(id) == head_id
    };
    Ok(pending_done
        .iter()
        .chain(pending_gate.iter())
        .chain(pending_edit.iter())
        .any(|raw| names_head(raw)))
}

pub(crate) fn explicit_queue_completion_ids(
    pending_done: &[String],
    pending_gate: &[String],
    pending_edit: &[String],
) -> Vec<String> {
    pending_done
        .iter()
        .chain(pending_gate.iter())
        .chain(pending_edit.iter())
        .map(|raw| {
            raw.split_once('=')
                .map(|(id, _)| id)
                .unwrap_or(raw.as_str())
        })
        .map(str::to_string)
        .collect()
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

pub(crate) fn queue_head_matches_done_ids(content: &str, done_ids: &[String]) -> Result<bool> {
    if done_ids.is_empty() {
        return Ok(false);
    }
    let Some(queue_head) = active_queue_head_text(content)? else {
        return Ok(false);
    };
    let Some(head_id) = queue_prompt_done_id(&queue_head) else {
        return Ok(false);
    };
    Ok(done_ids.iter().any(|id| normalize_done_id(id) == head_id))
}

pub(crate) fn queue_prompt_text_matches(prompt_change: &str, queue_head: &str) -> bool {
    normalize_queue_prompt_text(prompt_change) == normalize_queue_prompt_text(queue_head)
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

pub(crate) fn response_explicitly_targets_queue_head(response: &str, queue_head: &str) -> bool {
    response
        .lines()
        .filter_map(response_heading_topic)
        .any(|topic| response_topic_matches_queue_head(topic, queue_head))
}

pub(crate) fn response_heading_topic(line: &str) -> Option<&str> {
    let trimmed = line.trim().trim_start_matches('❯').trim();
    let topic = trimmed.strip_prefix("### Re:")?.trim();
    Some(
        topic
            .split_once(" — ")
            .map(|(topic, _)| topic)
            .unwrap_or(topic)
            .trim(),
    )
}

pub(crate) fn response_topic_matches_queue_head(topic: &str, queue_head: &str) -> bool {
    // Used by the Codex Stop-hook auto-close path, which has no closeout CLI flags
    // to express completion explicitly. Two completion shapes count:
    //  1. An exact topic match (`### Re: do [#foo]` vs head `do [#foo]`).
    //  2. A topic that resolves to EXACTLY the head id (`### Re: #fix1` vs head
    //     `do #fix1`) — the Codex auto-loop titles a clean completion with the
    //     head's `#id` (#queue-head-consume-on-topic-id-regression).
    // A heading topic that merely contains the head id with trailing modifiers —
    // `### Re: #id halt`, `### Re: #id deferred` — must NOT count as completion
    // (#queue-strike-on-halt); `topic_resolves_to_exact_id` rejects those.
    if queue_prompt_text_matches(topic, queue_head) {
        return true;
    }
    queue_prompt_done_id(queue_head)
        .is_some_and(|head_id| topic_resolves_to_exact_id(topic, &head_id))
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

/// True when `head_id` matches a registered `prompt_presets` key in the
/// document frontmatter and does NOT also name a tracked backlog/review/icebox
/// directive item (#qpresetstrike).
///
/// A queue head that is a bare prompt-preset token (`#advance-review` drawn from
/// frontmatter `prompt_presets`) has no backlog row, so no `--done`/`--pending-gate`
/// closeout can record a completion outcome for it: `--done advance-review` fails
/// with "id not found in backlog/icebox", and the leading `#` made the old
/// classifier route `queue consume` to `--done` and wedge the head. Such a head is
/// a *synthetic* prompt completed by being answered (like a free-text head), so it
/// is strikeable through `queue consume` and the free-text finalize heuristic. A
/// preset token that ALSO happens to be a tracked backlog/review id stays id-backed
/// (the tracked-item check wins) so it keeps its explicit `--done` reap path.
pub(crate) fn head_id_is_registered_preset(content: &str, head_id: &str) -> bool {
    if head_id_names_tracked_directive_item(content, head_id) {
        return false;
    }
    let Ok((fm, _)) = frontmatter::parse(content) else {
        return false;
    };
    crate::frontmatter::resolve_prompt_preset_key(&fm.prompt_presets, head_id).is_some()
}

/// True when `head_id` names an item tracked in `agent:backlog` or `agent:review`.
/// These ids are id-backed directives requiring an explicit
/// `--done`/`--pending-gate`/`--pending-edit` closeout — distinct from a
/// registered prompt-preset id (e.g. `#spec-...`), which is not a tracked item and
/// still completes on a `### Re: #id` heading match (#zwn5).
pub(crate) fn head_id_names_tracked_directive_item(content: &str, head_id: &str) -> bool {
    let Ok(comps) = crate::component::parse(content) else {
        return false;
    };
    comps
        .iter()
        .filter(|c| c.name == "backlog" || c.name == "review" || c.name == "pending")
        .any(|comp| {
            let (_, items, _) = crate::pending::parse_items(comp.content(content));
            items
                .iter()
                .any(|item| !item.id.is_empty() && item.id.eq_ignore_ascii_case(head_id))
        })
}

/// A queue head that is just a `do [#id]` / `do #id` directive — the `do` verb
/// plus the id (with optional bracket sugar) and nothing else. These follow the
/// strike-on-halt explicit-flag rule rather than heading-based consumption.
pub(crate) fn queue_head_is_bare_do_directive(queue_head: &str) -> bool {
    let norm = normalize_queue_prompt_text(queue_head);
    let Some(rest) = norm.strip_prefix("do ") else {
        return false;
    };
    matches!(
        rest.strip_prefix('#'),
        Some(id)
            if !id.is_empty()
                && id
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    )
}

/// True when the active queue head is a free-text prompt: it carries no
/// extractable `#id` (so it is neither a `do [#id]` directive nor a `#preset`
/// head) and is not a `do queue` / `run queue` activation trigger. Such a prompt
/// has no `#id`-based completion mechanism — none of the explicit-flag or
/// heading-id consumption paths can ever strike it — so it is consumed by being
/// answered: a captured response body for the cycle completes it
/// (#free-text-queue-head-consume).
pub(crate) fn queue_head_is_free_text_prompt(content: &str) -> Result<bool> {
    let Some(queue_head) = active_queue_head_text(content)? else {
        return Ok(false);
    };
    // #free-text-queue-owner-consume: a head is id-backed (NOT free text, so it
    // needs an explicit `--done`/`--pending-gate`/`--pending-edit` completion
    // signal) only when the ENTIRE head resolves to a single id directive —
    // `#id`, `[#id]`, or `do [#id]`. A free-text head that merely *mentions* a
    // `#id` in prose — e.g. `Approve [#shoptiers]. What are #next-steps?` — is
    // still free text and completes on being answered. The old `queue_prompt_done_id(..).is_some()`
    // test matched any `#id` mention and wrongly left such heads un-strikable,
    // hanging the auto-queue (they have no single id to `--done`).
    let normalized_head = normalize_queue_prompt_text(&queue_head);
    if let Some(id) = queue_prompt_done_id(&normalized_head)
        && topic_resolves_to_exact_id(&normalized_head, &id)
    {
        // #qpresetstrike: a head that resolves to exactly a registered
        // `prompt_presets` token (and is not also a tracked backlog/review id) has
        // no `--done` reap path — it is a synthetic prompt completed by being
        // answered, so treat it as free text (strikeable by `queue consume` and the
        // free-text finalize heuristic) rather than wedging it as id-backed.
        if head_id_is_registered_preset(content, &id) {
            return Ok(true);
        }
        return Ok(false);
    }
    if crate::diff::detect_queue_trigger(&normalized_head) {
        return Ok(false);
    }
    Ok(true)
}

/// True when `text` is a free-text queue prompt (NOT an id-backed directive and
/// NOT a queue trigger) — the per-entry analogue of
/// [`queue_head_is_free_text_prompt`], used by the position-independent
/// answered-head strike (#ftstrike) which must classify entries anywhere in the
/// queue, not only the active head.
pub(crate) fn queue_prompt_text_is_free_text(content: &str, text: &str) -> bool {
    let normalized = normalize_queue_prompt_text(text);
    if let Some(id) = queue_prompt_done_id(&normalized)
        && topic_resolves_to_exact_id(&normalized, &id)
    {
        // Mirrors `queue_head_is_free_text_prompt`: a head resolving to exactly a
        // registered preset token has no `--done` reap path, so it is free text;
        // any other exact-id directive is id-backed.
        return head_id_is_registered_preset(content, &id);
    }
    !crate::diff::detect_queue_trigger(&normalized)
}

/// Collapse a string to lowercase alphanumeric words separated by single spaces.
/// Every non-alphanumeric run (`:pushpin:`, `- `, backticks, punctuation,
/// newlines) becomes one space, so two spellings of the same prompt compare equal
/// regardless of cosmetic markers (#ftstrike).
fn normalize_for_answer_match(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            for lc in ch.to_lowercase() {
                out.push(lc);
            }
            prev_space = false;
        } else if !prev_space && !out.is_empty() {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim().to_string()
}

/// The concatenated, normalized text of every `>` blockquote line in the response.
/// The skill quotes the prompt it is answering as a blockquote (`> **Queue
/// prompt:**` / `> <text>`), so a free-text head is "answered" only when its text
/// appears in this quoted region — prose that merely *mentions* a head (without
/// quoting it as a prompt) does NOT count, which keeps an unaddressed operator
/// report from being silently struck (#ftstrike false-strike guard).
fn response_blockquote_text(response_body: &str) -> String {
    let joined = response_body
        .lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with('>'))
        .map(|line| line.trim_start_matches('>'))
        .collect::<Vec<_>>()
        .join(" ");
    normalize_for_answer_match(&joined)
}

/// True when the committed `response_body` answers the free-text queue head
/// `head_text`: its normalized text appears inside the response's quoted-prompt
/// blockquote region. Requires a head of at least four significant words so a
/// short/empty head cannot match incidentally — the conservative direction,
/// because a false positive silently drops an unaddressed operator report.
pub(crate) fn free_text_head_answered_by_response(response_body: &str, head_text: &str) -> bool {
    // Strip the leading operator/agent pin (`:pushpin:` …) first — its literal
    // shortcode word would otherwise survive normalization and break the match.
    let head_clean = crate::queue::strip_priority_markers(head_text);
    let head_norm = normalize_for_answer_match(&head_clean);
    if head_norm.split(' ').filter(|w| !w.is_empty()).count() < 4 {
        return false;
    }
    response_blockquote_text(response_body).contains(&head_norm)
}

/// Node keys of every non-struck free-text queue head that `response_body`
/// answers, at ANY position in the queue (#ftstrike). Mirrors how
/// `strike_done_queue_head_prompts` strikes id-backed heads regardless of
/// position, but matches free-text heads to the answering response instead of a
/// tracked id outcome.
pub(crate) fn answered_free_text_head_node_keys(
    content: &str,
    response_body: &str,
) -> Result<Vec<String>> {
    if response_body.trim().is_empty() {
        return Ok(Vec::new());
    }
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
        .map_err(|err| anyhow::anyhow!("free-text strike: failed to derive queue node keys: {err}"))?;
    let mut keys = Vec::new();
    for node in nodes {
        if node.item.struck {
            continue;
        }
        let text = node.item.text.trim();
        if text.is_empty() || !queue_prompt_text_is_free_text(content, text) {
            continue;
        }
        if free_text_head_answered_by_response(response_body, text) {
            keys.push(node.node_key);
        }
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
    let keys = answered_free_text_head_node_keys(&content, response_body)?;
    if keys.is_empty() {
        return Ok(0);
    }
    let new_document = consume_queue_nodes_by_key(&content, &keys)?;
    if new_document == content {
        return Ok(0);
    }

    // Snapshot sync: match the same answered free-text heads in the snapshot and
    // strike them by the snapshot's own node keys (keys are position/hash derived
    // and need not equal the document's). Required closeouts must prove both sides
    // converge on the struck state.
    let new_snapshot = match snapshot::load(file)? {
        Some(snap) => {
            let snap_keys = answered_free_text_head_node_keys(&snap, response_body)?;
            if snap_keys.is_empty() {
                None
            } else {
                Some(consume_queue_nodes_by_key(&snap, &snap_keys)?)
            }
        }
        None => None,
    };

    if skip_visible_guard {
        atomic_write(file, &new_document)
            .context("free-text strike: failed to write document")?;
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
    Ok(keys.len())
}

/// Resolve whether this cycle's committed response should consume (strike) the
/// active queue head. Single source of truth for the strict-closeout decision so
/// successful closeouts advance the queue identically and never leave an answered
/// head queued to treadmill the auto-loop on the next preflight. Unproven IPC
/// attempts fail before queue consumption and must be retried.
///
/// Mirrors the layered signals: explicit `do queue` / prompt-target / `--done`
/// triggers, explicit `--done`/`--pending-gate`/`--pending-edit` completion of an
/// id-backed head, a response heading that resolves to a synthetic/preset head
/// id, and a free-text head answered by this cycle's response (unless the cycle
/// answered a foreign `agent:exchange` prompt instead).
pub(crate) fn queue_consumption_allowed_for_response(
    file: &Path,
    baseline: Option<&str>,
    current_content: &str,
    response_body: &str,
    pending_done: &[String],
    pending_gate: &[String],
    pending_edit: &[String],
) -> Result<bool> {
    if should_consume_queue_prompt_for_write(file, baseline, current_content, pending_done)? {
        return Ok(true);
    }
    if queue_head_has_explicit_completion_signal(
        current_content,
        pending_done,
        pending_gate,
        pending_edit,
    )? {
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
        return Ok(!cycle_answered_foreign_exchange_prompt(
            baseline,
            current_content,
            &head_text,
        ));
    }
    Ok(false)
}

/// True when `topic` resolves to exactly `#<head_id>` (optionally `do `-prefixed
/// or `[#id]` bracketed) with no trailing modifiers. Case-insensitive; `head_id`
/// is already normalized lowercase by [`queue_prompt_done_id`].
pub(crate) fn topic_resolves_to_exact_id(topic: &str, head_id: &str) -> bool {
    let norm = topic.trim().trim_start_matches('❯').trim();
    let norm = norm.strip_prefix("do ").unwrap_or(norm).trim();
    let inner = norm
        .strip_prefix("[#")
        .and_then(|rest| rest.strip_suffix(']'))
        .or_else(|| norm.strip_prefix('#'));
    matches!(inner, Some(id) if id.eq_ignore_ascii_case(head_id))
}

pub(crate) fn queue_prompt_done_id(text: &str) -> Option<String> {
    let marker = text.find('#')?;
    let tail = &text[marker + 1..];
    let id = tail
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect::<String>();
    if id.is_empty() {
        None
    } else {
        Some(id.to_ascii_lowercase())
    }
}

pub(crate) fn normalize_done_id(id: &str) -> String {
    id.trim()
        .trim_start_matches('[')
        .trim_start_matches('#')
        .trim_end_matches(']')
        .to_ascii_lowercase()
}

pub(crate) fn first_n_queue_prompt_texts(
    entries: &[crate::queue::QueueEntry],
    count: usize,
) -> Vec<String> {
    entries
        .iter()
        .filter_map(|entry| match entry {
            crate::queue::QueueEntry::Prompt(prompt) => Some(prompt.text.clone()),
            _ => None,
        })
        .take(count)
        .collect()
}

pub(crate) fn queue_consume_count_for_done_ids(
    entries: &[crate::queue::QueueEntry],
    done_ids: &[String],
) -> usize {
    if done_ids.is_empty() {
        return 0;
    }
    let done_ids = done_ids
        .iter()
        .map(|id| normalize_done_id(id))
        .collect::<std::collections::HashSet<_>>();
    let mut count = 0usize;
    for entry in entries {
        let crate::queue::QueueEntry::Prompt(prompt) = entry else {
            continue;
        };
        let Some(id) = queue_prompt_done_id(&prompt.text) else {
            break;
        };
        if done_ids.contains(&id) {
            count += 1;
            continue;
        }
        break;
    }
    count
}

pub(crate) fn mark_entries_completed_by_done_ids(
    entries: &[crate::queue::QueueEntry],
    done_ids: &[String],
) -> (Vec<crate::queue::QueueEntry>, Vec<String>) {
    if done_ids.is_empty() {
        return (entries.to_vec(), Vec::new());
    }
    let done_ids = done_ids
        .iter()
        .map(|id| normalize_done_id(id))
        .collect::<std::collections::HashSet<_>>();
    let mut marked_texts = Vec::new();
    let entries = entries
        .iter()
        .map(|entry| match entry {
            crate::queue::QueueEntry::Prompt(prompt)
                if queue_prompt_done_id(&prompt.text).is_some_and(|id| done_ids.contains(&id)) =>
            {
                marked_texts.push(prompt.text.clone());
                crate::queue::QueueEntry::Completed(prompt.clone())
            }
            _ => entry.clone(),
        })
        .collect();
    (entries, marked_texts)
}

pub(crate) fn normalized_done_id_bag(texts: &[String]) -> Vec<String> {
    let mut ids = texts
        .iter()
        .filter_map(|text| queue_prompt_done_id(text))
        .collect::<Vec<_>>();
    ids.sort();
    ids
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
    let components = component::parse(&content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(0);
    };
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries =
        crate::queue::parse(body).context("queue done-id mark: failed to parse document queue")?;
    let (marked_entries, marked_texts) = mark_entries_completed_by_done_ids(&entries, done_ids);
    if marked_texts.is_empty() {
        return Ok(0);
    }

    let new_body = crate::queue::render(&marked_entries);
    let new_document = queue_component.replace_content(&content, &new_body);

    let new_snapshot = if let Some(snapshot_content) = snapshot::load(file)? {
        let snapshot_components = component::parse(&snapshot_content)?;
        let snapshot_queue = snapshot_components
            .iter()
            .find(|component| component.name == "queue")
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "queue done-id mark: document queue changed but snapshot has no agent:queue component"
                )
            })?;
        let snapshot_body = &snapshot_content[snapshot_queue.open_end..snapshot_queue.close_start];
        let snapshot_entries = crate::queue::parse(snapshot_body)
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
        let snapshot_body = crate::queue::render(&snapshot_marked_entries);
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

    let components = component::parse(content)?;
    let queue_component = components
        .iter()
        .find(|c| c.name == "queue")
        .ok_or_else(|| anyhow::anyhow!("queue consume: document has no agent:queue component"))?;
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries =
        crate::queue::parse(body).context("queue consume: failed to parse document queue")?;
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
            let hash = crate::ops_log::content_hash(text);
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
            let hash = crate::ops_log::content_hash(text);
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
    agent_doc_markdown_ast::mutations::consume_nodes(content, "queue", &borrowed)
        .map_err(|err| anyhow::anyhow!("queue consume: failed to apply node-keyed consume: {err}"))
}

pub(crate) fn normalize_queue_prompt_text(text: &str) -> String {
    display_queue_prompt_text(text).to_ascii_lowercase()
}

pub(crate) fn display_queue_prompt_text(text: &str) -> String {
    text.lines()
        .map(|line| {
            let line = line.trim().trim_start_matches('❯').trim();
            crate::queue::strip_priority_markers(line)
                .replace("[#", "#")
                .replace(']', "")
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// First non-empty, trimmed line of `text`, or `None` when blank.
pub(crate) fn first_nonempty_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|l| !l.is_empty())
}

/// Format consumed queue prompt(s) as a labeled blockquote echo so the response
/// block records the prompt it answered (#queue-prompt-echo-in-response).
///
/// `max_chars` is the opt-in `#queue-prompt-echo-summary` threshold: when
/// `Some(n)` and a prompt exceeds `n` characters, the echo records a bounded
/// summary (first line truncated + elided-char count + a pointer to the full
/// `agent:queue` text) instead of the verbatim prompt. `None` (default)
/// preserves the verbatim copy the user asked to keep "for now".
pub(crate) fn format_consumed_prompt_echo(
    consumed_texts: &[String],
    max_chars: Option<usize>,
) -> String {
    let mut out = String::from("> **Queue prompt:**\n>\n");
    let mut first_block = true;
    for text in consumed_texts {
        if text.trim().is_empty() {
            continue;
        }
        if !first_block {
            out.push_str(">\n");
        }
        first_block = false;
        let rendered = match max_chars {
            Some(limit) if text.chars().count() > limit => summarize_consumed_prompt(text, limit),
            _ => text.clone(),
        };
        for line in rendered.lines() {
            if line.trim().is_empty() {
                out.push_str(">\n");
            } else {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// `#queue-prompt-echo-summary`: a bounded one-line summary of a long consumed
/// queue prompt — its first non-empty line truncated to `limit` characters on a
/// char boundary, plus how many characters were elided and a pointer to the full
/// text preserved in `agent:queue`.
pub(crate) fn summarize_consumed_prompt(text: &str, limit: usize) -> String {
    let total = text.chars().count();
    let first = first_nonempty_line(text).unwrap_or("").trim();
    let head: String = first.chars().take(limit).collect();
    let elided = total.saturating_sub(head.chars().count());
    format!("{head}… (+{elided} more chars; full prompt retained in agent:queue)")
}

pub(crate) fn line_is_response_heading(trimmed: &str) -> bool {
    trimmed == "## Assistant"
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
        || trimmed.starts_with("###### Re:")
}

/// Normalize a prompt line for "already present in exchange" comparison:
/// trim and strip a leading `❯` prompt marker.
pub(crate) fn normalize_prompt_line(line: &str) -> String {
    line.trim().trim_start_matches('❯').trim().to_string()
}

/// Locate, within `region` (the exchange content), the byte offset of the line
/// where this cycle's response heading begins. Prefers the captured response's
/// first line; falls back to the last non-code `### Re:` heading. `region_base`
/// is the absolute offset of `region` within the full document, used to skip
/// matches inside fenced code blocks.
pub(crate) fn locate_response_heading_offset(
    region: &str,
    region_base: usize,
    response_first_line: Option<&str>,
    code_ranges: &[(usize, usize)],
) -> Option<usize> {
    let in_code = |rel: usize| {
        let abs = region_base + rel;
        code_ranges.iter().any(|&(cs, ce)| abs >= cs && abs < ce)
    };

    if let Some(target) = response_first_line.map(str::trim).filter(|t| !t.is_empty()) {
        let mut offset = 0usize;
        for line in region.split_inclusive('\n') {
            if line.trim() == target && !in_code(offset) {
                return Some(offset);
            }
            offset += line.len();
        }
    }

    let mut offset = 0usize;
    let mut found = None;
    for line in region.split_inclusive('\n') {
        if line_is_response_heading(line.trim()) && !in_code(offset) {
            found = Some(offset);
        }
        offset += line.len();
    }
    found
}

/// Embed the consumed queue prompt echo immediately after this cycle's response
/// heading inside the `exchange` component. Returns `content` unchanged (fail-safe)
/// when the exchange/heading cannot be located, the prompt is empty, or the prompt
/// already appears in the exchange (e.g. a user typed it in directly).
pub(crate) fn embed_consumed_prompt_in_response(
    content: &str,
    consumed_texts: &[String],
    response_first_line: Option<&str>,
) -> String {
    if consumed_texts.iter().all(|t| t.trim().is_empty()) {
        return content.to_string();
    }
    let Ok(components) = component::parse(content) else {
        return content.to_string();
    };
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return content.to_string();
    };
    let region = &content[exchange.open_end..exchange.close_start];

    // Idempotency / manual-turn dedup: if the prompt's first line already appears
    // as an exchange line (user typed it, or a prior echo exists), skip injection.
    // #queue-prompt-echo-summary: the opt-in length threshold is read from the
    // document's own frontmatter (default None = verbatim copy).
    let max_chars = frontmatter::parse(content)
        .ok()
        .and_then(|(fm, _)| fm.queue_prompt_echo_max_chars);
    let echo = format_consumed_prompt_echo(consumed_texts, max_chars);
    if region.contains(echo.trim_end()) {
        return content.to_string();
    }
    let already_present = consumed_texts
        .iter()
        .filter_map(|t| first_nonempty_line(t))
        .any(|first| {
            let needle = normalize_prompt_line(first);
            !needle.is_empty() && region.lines().any(|l| normalize_prompt_line(l) == needle)
        });
    if already_present {
        return content.to_string();
    }

    let code_ranges = component::find_code_ranges(content);
    let Some(heading_rel) = locate_response_heading_offset(
        region,
        exchange.open_end,
        response_first_line,
        &code_ranges,
    ) else {
        return content.to_string();
    };
    let Some(nl) = region[heading_rel..].find('\n') else {
        return content.to_string();
    };
    let insert_abs = exchange.open_end + heading_rel + nl + 1;

    let mut result = String::with_capacity(content.len() + echo.len() + 2);
    result.push_str(&content[..insert_abs]);
    result.push('\n');
    result.push_str(&echo);
    result.push('\n');
    result.push_str(&content[insert_abs..]);
    result
}

pub(crate) fn plan_queue_prompt_consumption(
    file: &Path,
    content: &str,
    done_ids: &[String],
) -> Result<Option<QueueConsumptionPlan>> {
    let (fm, _) = frontmatter::parse(content)?;
    if fm.queue_active != Some(true) {
        return Ok(None);
    }

    let components = component::parse(content)?;
    let comp = components
        .iter()
        .find(|c| c.name == "queue")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "queue consume: queue_active is true but document has no agent:queue component"
            )
        })?;

    let body = &content[comp.open_end..comp.close_start];
    let entries =
        crate::queue::parse(body).context("queue consume: failed to parse document queue")?;

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

            let has_auto = crate::queue::has_auto_attr(&comp.attrs);
            let remaining = crate::queue::prompts(&completed_entries).len();
            let drained = remaining == 0;
            let new_entries = if drained {
                Vec::new()
            } else {
                completed_entries
            };
            let new_body = crate::queue::render(&new_entries);
            let mut current = if drained || !consumed_node_keys.ast_backed {
                comp.replace_content(content, &new_body)
            } else {
                consume_queue_nodes_by_key(content, &consumed_node_keys.keys)?
            };

            if drained {
                if has_auto {
                    let comps = component::parse(&current)?;
                    if let Some(q) = comps.iter().find(|c| c.name == "queue") {
                        let raw = &current[q.open_start..q.open_end];
                        let new_tag = crate::queue::strip_auto_from_tag(raw);
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

            let snap = snapshot::load(file)?.ok_or_else(|| {
                anyhow::anyhow!("queue consume: queue_active is true but snapshot is missing")
            })?;
            let snap_comps = component::parse(&snap)?;
            let snap_queue = snap_comps
                .iter()
                .find(|c| c.name == "queue")
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "queue consume: queue_active is true but snapshot has no agent:queue component"
                    )
                })?;
            let snap_body = &snap[snap_queue.open_end..snap_queue.close_start];
            let snap_entries = crate::queue::parse(snap_body)
                .context("queue consume: failed to parse snapshot queue")?;
            let snap_has_auto = crate::queue::has_auto_attr(&snap_queue.attrs);
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
                queue_prompt_node_keys_for_done_ids(&snap, done_ids, &snapshot_consumed_texts);
            let snap_remaining = crate::queue::prompts(&snap_completed_entries).len();
            let snap_new_entries = if snap_remaining == 0 {
                Vec::new()
            } else {
                snap_completed_entries
            };
            if snap_new_entries != new_entries {
                let snap_remaining_prompts = crate::queue::prompts(&snap_new_entries).len();
                let doc_remaining_prompts = crate::queue::prompts(&new_entries).len();
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
                    snap_queue.replace_content(&snap, &new_body)
                } else {
                    consume_queue_nodes_by_key(&snap, &snapshot_node_keys.keys)?
                };
            if drained {
                if snap_has_auto
                    && let Ok(sc2) = component::parse(&new_snap)
                    && let Some(sq2) = sc2.iter().find(|c| c.name == "queue")
                {
                    let raw = &new_snap[sq2.open_start..sq2.open_end];
                    let new_tag = crate::queue::strip_auto_from_tag(raw);
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
    let consumed_texts = first_n_queue_prompt_texts(&entries, consume_count);
    let consumed_text = consumed_texts.first().cloned().ok_or_else(|| {
        anyhow::anyhow!(
            "queue consume: queue_active is true but document queue has no prompt to consume"
        )
    })?;
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
    let consumed_node_keys = queue_prompt_node_keys_for_count(content, consume_count)?;
    let node_ops = consumed_node_keys
        .keys
        .iter()
        .cloned()
        .map(|node_key| IpcNodeOp::consume("queue", node_key))
        .collect::<Vec<_>>();

    let has_auto = crate::queue::has_auto_attr(&comp.attrs);
    let completed_entries = crate::queue::mark_first_n_prompts_completed(&entries, consume_count);
    let remaining = crate::queue::prompts(&completed_entries).len();
    let drained = remaining == 0;
    let new_entries = if drained {
        Vec::new()
    } else {
        completed_entries
    };
    let new_body = crate::queue::render(&new_entries);
    let mut current = if drained || !consumed_node_keys.ast_backed {
        comp.replace_content(content, &new_body)
    } else {
        consume_queue_nodes_by_key(content, &consumed_node_keys.keys)?
    };

    if drained {
        if has_auto {
            let comps = component::parse(&current)?;
            if let Some(q) = comps.iter().find(|c| c.name == "queue") {
                let raw = &current[q.open_start..q.open_end];
                let new_tag = crate::queue::strip_auto_from_tag(raw);
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

    // Update snapshot in sync. Required closeouts must be able to prove the
    // same head prompt was removed from both the file and the snapshot.
    let snap = snapshot::load(file)?.ok_or_else(|| {
        anyhow::anyhow!("queue consume: queue_active is true but snapshot is missing")
    })?;
    let snap_comps = component::parse(&snap)?;
    let snap_queue = snap_comps
        .iter()
        .find(|c| c.name == "queue")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "queue consume: queue_active is true but snapshot has no agent:queue component"
            )
        })?;
    let snap_body = &snap[snap_queue.open_end..snap_queue.close_start];
    let snap_entries =
        crate::queue::parse(snap_body).context("queue consume: failed to parse snapshot queue")?;
    let snap_has_auto = crate::queue::has_auto_attr(&snap_queue.attrs);
    let snapshot_consumed_texts = first_n_queue_prompt_texts(&snap_entries, consume_count);
    let snapshot_node_keys = queue_prompt_node_keys_for_count(&snap, consume_count)?;
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
            .map(|t| crate::queue::strip_priority_markers(t))
            .collect::<Vec<_>>()
    };
    if norm(&snapshot_consumed_texts) != norm(&consumed_texts) {
        anyhow::bail!(
            "queue consume: snapshot head prompts {:?} do not match document head prompts {:?}",
            snapshot_consumed_texts,
            consumed_texts
        );
    }
    let snap_completed_entries =
        crate::queue::mark_first_n_prompts_completed(&snap_entries, consume_count);
    let snap_remaining = crate::queue::prompts(&snap_completed_entries).len();
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
        let snap_remaining_prompts = crate::queue::prompts(&snap_new_entries).len();
        let doc_remaining_prompts = crate::queue::prompts(&new_entries).len();
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
            snap_queue.replace_content(&snap, &new_body)
        } else {
            consume_queue_nodes_by_key(&snap, &snapshot_node_keys.keys)?
        };
    if drained {
        if snap_has_auto
            && let Ok(sc2) = component::parse(&new_snap)
            && let Some(sq2) = sc2.iter().find(|c| c.name == "queue")
        {
            let raw = &new_snap[sq2.open_start..sq2.open_end];
            let new_tag = crate::queue::strip_auto_from_tag(raw);
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
mod queue_prompt_echo_summary_tests {
    use super::*;

    #[test]
    fn echo_copies_verbatim_when_threshold_is_none() {
        // #queue-prompt-echo-summary: default (None) preserves the verbatim copy
        // the user asked to keep "for now".
        let long = "do [#x] ".to_string() + &"word ".repeat(100);
        let echo = format_consumed_prompt_echo(std::slice::from_ref(&long), None);
        assert!(echo.starts_with("> **Queue prompt:**\n>\n"));
        assert!(echo.contains(long.trim_end()));
        assert!(!echo.contains("more chars"));
    }

    #[test]
    fn echo_copies_verbatim_when_under_threshold() {
        let short = "do [#x] short prompt".to_string();
        let echo = format_consumed_prompt_echo(std::slice::from_ref(&short), Some(200));
        assert!(echo.contains("> do [#x] short prompt"));
        assert!(!echo.contains("more chars"));
    }

    #[test]
    fn echo_summarizes_when_over_threshold() {
        let long = "First line is the gist.\n".to_string() + &"tail ".repeat(100);
        let echo = format_consumed_prompt_echo(std::slice::from_ref(&long), Some(40));
        // The verbatim tail must NOT appear; a bounded summary must.
        assert!(!echo.contains(&"tail ".repeat(100)));
        assert!(echo.contains("First line is the gist."));
        assert!(echo.contains("more chars; full prompt retained in agent:queue"));
        // Summary is a single quoted line plus the label.
        assert_eq!(echo.matches("more chars").count(), 1);
    }

    #[test]
    fn summarize_truncates_first_line_on_char_boundary() {
        // Multibyte content must not panic and must truncate on a char boundary.
        let text = "héllo wörld ".repeat(20);
        let summary = summarize_consumed_prompt(&text, 5);
        assert!(summary.starts_with("héllo"));
        assert!(summary.contains("more chars"));
    }
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
    fn explicit_signal_halt_without_flag_does_not_consume() {
        // (a) Halt response, no --done/--pending-gate/--pending-edit → no consume.
        assert!(
            !queue_head_has_explicit_completion_signal(
                crate::test_support::HALT_QUEUE_DOC,
                &[],
                &[],
                &[]
            )
            .unwrap()
        );
    }
    #[test]
    fn explicit_signal_done_flag_consumes() {
        // (b) --done naming the head → consume. (c) also covers no-heading + --done.
        assert!(
            queue_head_has_explicit_completion_signal(
                crate::test_support::HALT_QUEUE_DOC,
                &["foo".to_string()],
                &[],
                &[],
            )
            .unwrap()
        );
    }
    #[test]
    fn explicit_signal_gate_and_edit_flags_consume() {
        assert!(
            queue_head_has_explicit_completion_signal(
                crate::test_support::HALT_QUEUE_DOC,
                &[],
                &["foo".to_string()],
                &[],
            )
            .unwrap(),
            "--pending-gate naming the head is a completion signal"
        );
        assert!(
            queue_head_has_explicit_completion_signal(
                crate::test_support::HALT_QUEUE_DOC,
                &[],
                &[],
                &["foo=rewritten text".to_string()],
            )
            .unwrap(),
            "--pending-edit naming the head is a completion signal"
        );
    }
    #[test]
    fn explicit_signal_flag_for_other_id_does_not_consume() {
        assert!(
            !queue_head_has_explicit_completion_signal(
                crate::test_support::HALT_QUEUE_DOC,
                &["bar".to_string()],
                &["baz".to_string()],
                &["qux=text".to_string()],
            )
            .unwrap(),
            "flags for non-head ids must not consume the head"
        );
    }
    #[test]
    fn explicit_signal_none_when_queue_inactive() {
        let inactive = crate::test_support::HALT_QUEUE_DOC
            .replace("queue_active: true", "queue_active: false");
        assert!(
            !queue_head_has_explicit_completion_signal(&inactive, &["foo".to_string()], &[], &[],)
                .unwrap()
        );
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
    fn done_id_marking_ignores_already_completed_queue_prompt() {
        let entries = crate::queue::parse(concat!(
            "- do [#head]\n",
            "- ~~do [#opportunistic]~~\n",
            "- do [#tail]\n",
        ))
        .unwrap();

        let (updated, marked) =
            mark_entries_completed_by_done_ids(&entries, &["opportunistic".to_string()]);
        assert!(marked.is_empty());
        assert_eq!(updated, entries);
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
        // the live repro head from src/boost-client/tasks/monsterrodholders.md.
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
        assert!(head_id_is_registered_preset(registered, "advance-review"));

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
        assert!(!head_id_is_registered_preset(
            preset_and_tracked,
            "advance-review"
        ));

        // An unregistered #-token (preset name not in frontmatter) is NOT treated
        // as a preset — it stays id-backed so it is never struck blind.
        let unregistered = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- #advance-review\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(!queue_head_is_free_text_prompt(unregistered).unwrap());
        assert!(!head_id_is_registered_preset(
            unregistered,
            "advance-review"
        ));
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
        // baseline == current (no new exchange prompt this cycle), non-empty
        // response → the free-text head is answered and must be consumed.
        assert!(
            queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: JB Run Agent Doc should start the queue\n\nFixed.",
                &[],
                &[],
                &[],
            )
            .unwrap(),
            "an answered free-text head must be consumed on successful closeout"
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
    fn free_text_head_answered_when_quoted_in_response_blockquote() {
        assert!(free_text_head_answered_by_response(
            FTSTRIKE_RESPONSE,
            "My free-text queue items are not immediately struck as if they are addressed."
        ));
        // Cosmetic differences (`:pushpin:`, leading `- `, backticks) must not matter.
        assert!(free_text_head_answered_by_response(
            FTSTRIKE_RESPONSE,
            ":pushpin: JB Run Agent Doc is stalled on this document when I tried to start the queue run. No notification."
        ));
    }

    #[test]
    fn free_text_head_not_struck_when_only_mentioned_in_prose() {
        // FALSE-STRIKE GUARD: a head whose text appears only in prose (not a `>`
        // quoted-prompt blockquote) must NOT be considered answered — otherwise an
        // unaddressed operator report would be silently struck/dropped.
        let prose_only = concat!(
            "### Re: something else — opus\n\n",
            "I noticed my free-text queue items are not immediately struck as if they are addressed, ",
            "but that is a different report I did not handle this turn.\n",
        );
        assert!(!free_text_head_answered_by_response(
            prose_only,
            "My free-text queue items are not immediately struck as if they are addressed."
        ));
    }

    #[test]
    fn free_text_head_too_short_is_not_matched() {
        let resp = "### Re: x\n\n> - fix it now\n";
        assert!(!free_text_head_answered_by_response(resp, "fix it now"));
    }

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
        let keys = answered_free_text_head_node_keys(content, FTSTRIKE_RESPONSE).unwrap();
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
    fn queue_prompt_text_is_free_text_classification() {
        let content = "---\nqueue_active: true\n---\n<!-- agent:queue -->\n- x\n<!-- /agent:queue -->\n";
        assert!(!queue_prompt_text_is_free_text(content, "do [#fullboundary]"));
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
        // Idempotent: a second pass strikes nothing more.
        let again = strike_answered_free_text_queue_heads(&doc, FTSTRIKE_RESPONSE, true).unwrap();
        assert_eq!(again, 0, "already-struck heads must not be re-struck");
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
                &[],
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
                &[],
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
                &[],
                &[],
            )
            .unwrap(),
            "--done naming the head id must consume it"
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
                &[],
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
                &[],
                &["ktw8".to_string()],
                &[],
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
    fn queue_skip_diagnostic_names_head_shape_and_repair_path() {
        let id_backed = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#foo]\n",
            "<!-- /agent:queue -->\n",
        );
        let id_message = queue_skip_diagnostic_for_content(id_backed).unwrap();
        assert!(id_message.contains("[queue] kept head `do #foo`"));
        assert!(id_message.contains("`--done foo`"));
        assert!(id_message.contains("`--pending-gate foo`"));
        assert!(id_message.contains("`--pending-edit \"foo=...\"`"));
        assert!(id_message.contains("missing proof"));

        let free_text = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- Review the queue diagnostics\n",
            "<!-- /agent:queue -->\n",
        );
        let free_text_message = queue_skip_diagnostic_for_content(free_text).unwrap();
        assert!(
            free_text_message
                .contains("[queue] kept free-text head `Review the queue diagnostics`")
        );
        assert!(free_text_message.contains("answered-response path"));
    }
    #[test]
    fn heading_topic_matches_head_exactly_or_by_exact_id() {
        // Codex Stop-hook path: exact-topic match, or a topic that resolves to
        // EXACTLY the head id (#queue-head-consume-on-topic-id-regression).
        assert!(response_topic_matches_queue_head("do [#foo]", "do [#foo]"));
        assert!(response_topic_matches_queue_head(
            "do [#foo]",
            ":pushpin: do [#foo]"
        ));
        assert!(response_topic_matches_queue_head("#fix1", "do #fix1"));
        assert!(response_topic_matches_queue_head("#foo", "do [#foo]"));
        // Halt/modifier headings must NOT count as completion (#queue-strike-on-halt).
        assert!(!response_topic_matches_queue_head("#foo halt", "do [#foo]"));
        assert!(!response_topic_matches_queue_head(
            "#foo deferred",
            "do [#foo]"
        ));
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
    fn topic_resolves_to_exact_id_rejects_modifiers() {
        assert!(topic_resolves_to_exact_id(
            "#spec-test-build-install-commit-push",
            "spec-test-build-install-commit-push"
        ));
        assert!(topic_resolves_to_exact_id("do [#foo]", "foo"));
        assert!(topic_resolves_to_exact_id("#Foo", "foo")); // case-insensitive
        // Trailing modifiers (#queue-strike-on-halt) must never resolve to the id.
        assert!(!topic_resolves_to_exact_id("#foo halt", "foo"));
        assert!(!topic_resolves_to_exact_id("#foo deferred", "foo"));
        assert!(!topic_resolves_to_exact_id("#other", "foo"));
    }
}
