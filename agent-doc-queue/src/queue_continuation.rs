//! Pure queue-continuation drainability policy.
//!
//! This module owns content-only decisions for active queue continuation,
//! drainable heads, deferred backlog ids, recurring imperative heads, and queue
//! noise classification. Callers own file IO, controller state, and marker persistence.

use std::collections::{HashMap, HashSet};

use agent_doc_document::queue_projection::{has_in_progress_marker, strip_in_progress_marker};
use agent_doc_element::element;
use agent_doc_element_backlog::backlog;
use agent_doc_frontmatter::frontmatter;
use anyhow::{Context, Result};

use crate::document_queue::{self, QueueEntry, QueuePrompt};
use crate::queue_claim::ClaimedQueueItems;

/// Shared non-stall guidance surfaced wherever `queue_continuation_required ==
/// true`. Centralizing the wording keeps preflight JSON
/// (`queue_continuation_guidance`) and `session-check` stdout in agreement
/// (`#degraded-ipc-no-stall`).
///
/// The failure this guards against: a `finalize` that reached `committed` +
/// `session-check ok` through a **proven CP editor delivery** is a successful
/// closeout; the in-session loop does not depend on any one connection attempt.
/// The agent must not invent a stop reason from a recovered-but-proven delivery.
/// Unproven IPC (`retry_without_disk_write`, missing response proof, no ack) or
/// a direct session-document disk fallback is different: it stops the loop until
/// the editor/CRDT path is retried and proven. The normal closeout states that
/// stop the loop are a FAILED closeout, an unproven IPC/delivery retry
/// condition, a `session-check` interruption, or a `lint-gate` block. Degraded
/// delivery recovery after proof, high session-accretion, a
/// `semantic_completion_match` warning, and a `[clean-session]` head wanting
/// "fresh context" are NOT stop reasons. A stale-binary supervisor is not a
/// passive keep-going condition either: recycle/yield at the boundary, then
/// continue on the fresh binary.
pub const CONTINUATION_NO_STALL_GUIDANCE: &str = "queue continuation required — keep draining after a proven closeout. A closeout that reached committed + session-check ok is successful after any proven CP editor delivery recovery: the in-session loop does not depend on one connection attempt. IPC timeout, missing response proof, recovery=retry_without_disk_write, or recovery=direct_write_fallback are not successful closeouts for an active editor buffer; retry the editor/CRDT path instead of using a direct session-document disk write. Only a failed closeout, unproven IPC/delivery retry condition, session-check interruption, or lint-gate block stops the loop. Recovered delivery after proof, high session-accretion, and semantic_completion_match warnings are NOT stop reasons. A stale-binary supervisor is a recycle/yield concern: follow recycle-yield or stale_install guidance so the supervisor recycles, then continue the drain on the fresh binary.";

/// `#wd40` / `#staleloop-recycle-restart` guidance surfaced when the route-owned
/// supervisor is running a stale binary and has asked the in-session loop to
/// YIELD one inter-item boundary so the supervisor can `execve`-hot-reload onto
/// the freshly-installed binary.
///
/// This is a *temporary, intentional* drop of `queue_continuation_required`, NOT
/// a drained queue or a stop reason: end the current turn cleanly (do not
/// re-trigger `agent-doc <FILE>`); the resulting idle boundary lets the stale
/// supervisor recycle, and the drain resumes automatically on the fresh binary.
/// Releasing the drain-owner lease (`agent-doc drain-claim <FILE> --release`)
/// hands the drain back to the freshly-recycled supervisor immediately instead
/// of waiting for the lease TTL.
pub const RECYCLE_YIELD_GUIDANCE: &str = "supervisor recycle pending — the route-owned supervisor is running a STALE agent-doc binary and has asked this loop to YIELD one boundary so it can hot-reload onto the freshly-installed binary. This is intentional and temporary, NOT a drained queue or a stop reason. End this turn cleanly (do NOT re-trigger agent-doc <FILE>); the idle boundary lets the stale supervisor execve-recycle and the drain resumes automatically on the fresh binary. Optionally run `agent-doc drain-claim <FILE> --release` to hand the drain back to the recycled supervisor immediately rather than waiting for the lease TTL. Do not run `make install` / `admin recycle` by hand — the supervisor now automates the recycle once you yield.";

/// Compose the binary-authoritative queue-continuation guidance, resolving the
/// `queue_paused` + `queue_continuation_required` "mixed signal" (`#qpausemix`).
///
/// When the queue is not controller-paused (`pause_reason == None`), this returns
/// the base [`CONTINUATION_NO_STALL_GUIDANCE`] verbatim. When an accepted
/// `admin queue pause` is in effect, it prepends a preamble that explicitly
/// states the two signals are NOT contradictory: the controller pause suppresses
/// only the *unattended* supervisor idle-watch auto-injection, while the attended
/// in-session loop remains the legitimate single-owner drain, and it surfaces
/// the recorded pause reason.
pub fn continuation_guidance(pause_reason: Option<&str>) -> String {
    match pause_reason {
        None => CONTINUATION_NO_STALL_GUIDANCE.to_string(),
        Some(reason) => {
            let reason = reason.trim();
            let reason_clause = if reason.is_empty() {
                "no reason recorded".to_string()
            } else {
                format!("recorded pause reason: {reason}")
            };
            format!(
                "queue_paused is set but is NOT a contradiction with queue_continuation_required — an accepted `admin queue pause` suppresses ONLY the unattended supervisor idle-watch auto-injection (the flood guard); the attended in-session loop remains the legitimate single-owner drain and must keep draining proven-closeout work. Do not stop the loop on the pause; to actually hold the in-session loop use `queue: pause` frontmatter or a `--- stop` fence, not the admin pause ({reason_clause}). {CONTINUATION_NO_STALL_GUIDANCE}"
            )
        }
    }
}

/// Effective continuation fields for preflight/session status output after
/// applying non-drain stop-or-yield policy supplied by orchestration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveContinuationOutput {
    pub required: bool,
    pub guidance: Option<String>,
}

/// Resolve queue continuation output after the caller supplies effect-derived
/// facts such as a pending supervisor recycle yield.
pub fn effective_continuation_output(
    raw_required: bool,
    recycle_yield_pending: bool,
    pause_reason: Option<&str>,
) -> EffectiveContinuationOutput {
    effective_continuation_output_with_preemption(
        raw_required,
        false,
        recycle_yield_pending,
        pause_reason,
    )
}

/// `#queueblockquoteintent`: a suppressed continuation must name its reason.
///
/// When an exchange prompt preempts the drain, the caller used to AND that into
/// `raw_required` before calling, so this returned `required: false` with
/// `guidance: None` — indistinguishable from a genuinely drained queue. Observed
/// 2026-08-09 on `tasks/agent-doc/agent-doc-bugs2.md`: `queue_active: true` with
/// four drainable heads, continuation suppressed, and BOTH `ui_outcome` and
/// `queue_continuation_guidance` null. The cause could only be found by reading
/// `user_intent_prompt_changes` by hand.
///
/// A stop with heads remaining is exactly the state that needs a reason, so pass
/// the preemption in rather than pre-folding it away.
pub fn effective_continuation_output_with_preemption(
    raw_required: bool,
    preempted_by_exchange_prompt: bool,
    recycle_yield_pending: bool,
    pause_reason: Option<&str>,
) -> EffectiveContinuationOutput {
    if recycle_yield_pending {
        return EffectiveContinuationOutput {
            required: false,
            guidance: Some(RECYCLE_YIELD_GUIDANCE.to_string()),
        };
    }
    if raw_required && !preempted_by_exchange_prompt {
        return EffectiveContinuationOutput {
            required: true,
            guidance: Some(continuation_guidance(pause_reason)),
        };
    }
    if raw_required && preempted_by_exchange_prompt {
        return EffectiveContinuationOutput {
            required: false,
            guidance: Some(EXCHANGE_PROMPT_PREEMPTION_GUIDANCE.to_string()),
        };
    }
    EffectiveContinuationOutput {
        required: false,
        guidance: None,
    }
}

/// Named reason for a drain suppressed by a fresh exchange prompt
/// (`#queueblockquoteintent`).
pub const EXCHANGE_PROMPT_PREEMPTION_GUIDANCE: &str = "queue continuation suppressed — drainable heads remain, but this cycle's diff carries a fresh exchange prompt that preempts the drain (`user_intent_prompt_changes` is non-empty). Answer that prompt this turn; the queue resumes on the next cycle. This is NOT a drained queue: if you cannot find an operator prompt in the exchange tail, inspect `user_intent_prompt_changes` — binary-authored bookkeeping misclassified as user intent is a defect, not a stop reason.";

/// `#bgclearpolicy`: named reason for an in-session drain that hands the item
/// boundary to the supervisor so the next head runs in a fresh agent context.
pub const QUEUE_AUTO_CLEAR_YIELD_GUIDANCE: &str = "queue auto-clear — drainable heads remain, and this queue resets agent context between items (the default, positively declared by bare `clear`; legacy `disable-auto-clear` turns it off). After this item's proven closeout, END THIS TURN: do not schedule `/loop`, do not continue in-pane, and do not re-trigger agent-doc <FILE>. The ready supervisor clears the context at the idle boundary (Claude Code `/clear` and OpenCode `/new` typed into the live process; Codex gets a fresh restart) and dispatches the next head to the fresh context. Subagent queues wait until in-flight claims settle before that idle boundary is safe. This is NOT a drained or stalled queue. A clear does not shrink the document: session-accretion `block` measures the exchange size, which only compaction reduces.";

/// `#bgclearpolicy`: apply the auto-clear hand-off to the effective
/// continuation. A yield turns a required continuation into a named,
/// intentional stop; every other output passes through unchanged.
pub fn apply_auto_clear_yield(
    output: EffectiveContinuationOutput,
    yields: bool,
) -> EffectiveContinuationOutput {
    if yields && output.required {
        return EffectiveContinuationOutput {
            required: false,
            guidance: Some(QUEUE_AUTO_CLEAR_YIELD_GUIDANCE.to_string()),
        };
    }
    output
}

/// Named reason for an active `start` queue that belongs to the supervisor,
/// rather than the attended in-session drain (`#queuestartscopesilent`).
pub const SUPERVISOR_SCOPED_ACTIVATION_GUIDANCE: &str = "queue continuation is supervisor-scoped — ready queue heads remain under `queue: start`, so the supervisor idle-watch owns the drain. This is NOT a stalled or drained queue. Set `queue: go` to hand the drain to the attended in-session loop.";

/// Explain the otherwise-silent state where in-session activation is absent,
/// but supervisor activation is present and still contains prompt entries.
pub fn supervisor_scoped_activation_guidance(content: &str) -> Option<String> {
    if active_queue(content).is_some() {
        return None;
    }
    let (_, activation) = active_queue_for_supervisor_start(content, true)?;
    if document_queue::prompts(&activation.entries_after).is_empty() {
        return None;
    }
    Some(SUPERVISOR_SCOPED_ACTIVATION_GUIDANCE.to_string())
}

/// A required queue continuation: the document closed cleanly, explicit `go`
/// mode is active, and a ready queue head remains, so an in-session loop must
/// continue draining instead of sending a final answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueContinuation {
    pub head_prompt: String,
    pub head_id: Option<String>,
    pub reason: String,
}

/// Detect whether already-read document content requires queue continuation.
///
/// `snapshot_content` is optional committed/baseline content supplied by the
/// caller. When present, a modified queue head suppresses continuation so the
/// normal preflight/halt path can handle the operator edit. This function is
/// pure content policy: callers own file IO, snapshot loading, recycle-yield
/// checks, controller pause state, and marker persistence.
pub fn required_continuation(
    content: &str,
    snapshot_content: Option<&str>,
) -> Result<Option<QueueContinuation>> {
    required_continuation_excluding_claimed(content, snapshot_content, &ClaimedQueueItems::none())
}

/// [`required_continuation`], skipping heads `claimed` by a worker outside the
/// in-session loop (`#queueclaim`). A claimed head is in flight elsewhere, so
/// it is not the in-session loop's next head; when every remaining head is
/// claimed (or deferred) no continuation is required.
pub fn required_continuation_excluding_claimed(
    content: &str,
    snapshot_content: Option<&str>,
    claimed: &ClaimedQueueItems,
) -> Result<Option<QueueContinuation>> {
    required_continuation_excluding_claimed_scoped(
        content,
        snapshot_content,
        claimed,
        &whole_exchange_head_entry,
    )
}

/// `#qheadresiduescope` / GH #327 pass 4: which echoes may answer the live
/// free-text head `head_text` of queue `queue_id`
/// ([`crate::queue_head_entry::HeadEchoScope`]), the same scope the preflight
/// catch-up residue strike resolves. Arguments: `(queue_id, head_text)`.
pub type HeadEchoScopeFor<'a> = dyn Fn(&str, &str) -> crate::queue_head_entry::HeadEchoScope + 'a;

/// A [`HeadEchoScopeFor`] with no history: every echo in the exchange counts.
pub fn whole_exchange_head_entry(
    _queue_id: &str,
    _head: &str,
) -> crate::queue_head_entry::HeadEchoScope {
    crate::queue_head_entry::HeadEchoScope::WholeExchange
}

/// [`required_continuation_excluding_claimed`] whose `#syntheticstrike`
/// answered-head skip counts only the echoes `head_echo_scope` admits: those
/// written after each head's entry point (`#qheadresiduescope`) and, for a
/// `loop` queue, in the current pass about this queue's copy, with none while
/// no pass is open (GH #327 pass 4). The file-backed adapter in
/// `agent-doc-queue-io` resolves the scope from the schedule state and the
/// document's git history, exactly as the preflight catch-up strike does.
pub fn required_continuation_excluding_claimed_scoped(
    content: &str,
    snapshot_content: Option<&str>,
    claimed: &ClaimedQueueItems,
    head_echo_scope: &HeadEchoScopeFor<'_>,
) -> Result<Option<QueueContinuation>> {
    required_continuation_excluding_claimed_scoped_with_presets(
        content,
        snapshot_content,
        claimed,
        head_echo_scope,
        &frontmatter::PromptPresets::default(),
    )
}

/// [`required_continuation_excluding_claimed_scoped`] with the document's
/// effective preset map, so a head naming a project-inherited preset is a
/// preset invocation rather than a stale backlog mirror (GH #381).
pub fn required_continuation_excluding_claimed_scoped_with_presets(
    content: &str,
    snapshot_content: Option<&str>,
    claimed: &ClaimedQueueItems,
    head_echo_scope: &HeadEchoScopeFor<'_>,
    presets: &frontmatter::PromptPresets,
) -> Result<Option<QueueContinuation>> {
    let (fm, _) = frontmatter::parse(content)?;
    let components = element::parse(content)?;
    let queue_set = crate::queue_set::QueueSet::parse(content, &components)?;
    let Some(selected_queue) = queue_set.selected() else {
        return Ok(None);
    };
    let queue_id = selected_queue.id.clone();
    let queue_component = selected_queue.component;
    // `#qbindingone` (GH #79): the resolved control binding is the only
    // activation authority. Requiring the legacy `queue_active: true`, which
    // current writers no longer emit, kept this detector silent for every
    // `queue: go` document (`#qstartinert` fixed the same flag elsewhere).
    let control = crate::control_binding::frontmatter_queue_control(&fm);
    // `#queuegodefaultdrain`: a queue with no control on either surface is in
    // its default `go`, so it continues exactly like an explicit `go`.
    if !crate::control_binding::explicit_queue_go_mode(&queue_component.attrs, control) {
        return Ok(None);
    }
    let has_auto = document_queue::has_auto_attr(&queue_component.attrs);
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries =
        document_queue::parse(body).context("queue continuation: failed to parse queue")?;
    let activation = document_queue::resolve_activation(&entries, has_auto, false, true);
    if !activation.active
        || document_queue::has_stop_fence_at_head(&activation.entries_after)
        || document_queue::time_gate_at_head(&activation.entries_after).is_some()
    {
        return Ok(None);
    }

    if let Some(snapshot_content) = snapshot_content
        && let Ok(snapshot_components) = element::parse(snapshot_content)
        && let Ok(Some(snapshot_queue)) =
            crate::queue_set::component_for_id(snapshot_content, &snapshot_components, &queue_id)
    {
        let snapshot_body = &snapshot_content[snapshot_queue.open_end..snapshot_queue.close_start];
        if let Ok(snapshot_entries) = document_queue::parse(snapshot_body) {
            let snapshot_has_auto = document_queue::has_auto_attr(&snapshot_queue.attrs);
            let snapshot_activation = document_queue::resolve_activation(
                &snapshot_entries,
                snapshot_has_auto,
                false,
                true,
            );
            if document_queue::detect_head_prompt_modified(
                &snapshot_activation.entries_after,
                &activation.entries_after,
            ) {
                return Ok(None);
            }
        }
    }

    let Some(head) = continuation_head_skipping_answered_residue(
        content,
        &queue_id,
        claimed,
        head_echo_scope,
        presets,
    ) else {
        return Ok(None);
    };
    // `🚧` is a transient UI projection, never part of queue identity or the
    // prompt handed back to a harness. Other live-head projections in this
    // module already normalize it; keep the continuation contract consistent.
    let head_prompt = if has_in_progress_marker(&head.text) {
        strip_in_progress_marker(&head.text)
    } else {
        head.text.clone()
    };
    let head_id = extract_head_id(&head_prompt);
    let reason = if queue_component.attrs.contains_key("go") {
        "active `agent:queue go` still has a ready head prompt after a clean closeout"
    } else if has_auto {
        "active `agent:queue auto go` still has a ready head prompt after a clean closeout"
    } else {
        "active `queue: go` still has a ready head prompt after a clean closeout"
    }
    .to_string();

    Ok(Some(QueueContinuation {
        reason,
        head_id,
        head_prompt,
    }))
}

/// `#syntheticstrike`: the drainable head for a post-closeout continuation,
/// skipping free-text heads the committed exchange already answered.
///
/// A closeout that commits under a synthetic cycle (its capture's cycle was
/// abandoned) cannot run the capture-owned answered-free-text strike, so the
/// answered head stays live until the next preflight's residue strike. Offering
/// it as the continuation head re-dispatched finished work (agent-doc-bugs.md
/// 2026-09-29 14:28: "Fix …/issues/59" came back as the next head). This uses
/// the same residue predicate as session-check and the preflight strike, so a
/// recurring imperative head is never skipped.
///
/// `#qheadresiduescope`: only an echo written after the head (re-)entered the
/// queue answers it, the same scope as the preflight catch-up strike. An
/// operator who re-types an already-answered report (or un-strikes it) has
/// asked for it again, so the earlier echo must not hide the new copy from the
/// continuation offer. The entry anchor is read only for a head the whole
/// exchange would call answered, so an unanswered head costs no history read.
///
/// GH #327 pass 4: a `loop` queue's head is answered only by an echo written
/// in its current pass about this queue's copy, and by none while no pass is
/// open, so a `loop` head the preflight residue strike keeps is never skipped
/// here as answered.
fn continuation_head_skipping_answered_residue(
    content: &str,
    queue_id: &str,
    claimed: &ClaimedQueueItems,
    head_echo_scope: &HeadEchoScopeFor<'_>,
    presets: &frontmatter::PromptPresets,
) -> Option<QueuePrompt> {
    let (queue_facts, activation) = active_queue_for_supervisor_start(content, false)?;
    let exchange_text = crate::queue_head_entry::exchange_component_text(content);
    let answered_residue = |head: &str| -> bool {
        crate::queue_heads::free_text_queue_head_is_completed_residue(content, &exchange_text, head)
            && head_echo_scope(queue_id, head).answers(&exchange_text, |scoped| {
                crate::queue_heads::free_text_queue_head_is_completed_residue(content, scoped, head)
            })
    };
    let entries: Vec<QueueEntry> = activation
        .entries_after
        .into_iter()
        .filter(|entry| match entry {
            QueueEntry::Prompt(prompt) => {
                !claimed.claims(&prompt.text) && !answered_residue(prompt.text.trim())
            }
            _ => true,
        })
        .collect();
    eligible_head_prompt_from_entries(
        content,
        &entries,
        queue_facts.preset_supplies_directive,
        DrainScope::InSessionLoop,
        &preset_only_identity_ids_with_presets(content, presets),
    )
}

/// Drain scope for computing which backlog ids are deferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainScope {
    /// In-session loop: defers `[operator-verify]` and `[focused-cycle]`.
    InSessionLoop,
    /// Supervisor clear-and-continue: defers `[operator-verify]` only.
    Supervisor,
}

/// Why a backlog id was skipped from an auto-drain queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklogDrainSkip {
    pub id: String,
    /// `operator_verify` or `focused_cycle`.
    pub reason: &'static str,
}

/// Partition backlog ids into in-session-loop drainable and skipped sets.
///
/// Missing ids in `contexts` are plain backlog items and remain drainable.
/// `[clean-session]` drains in-loop; `[operator-verify]` and `[focused-cycle]`
/// are deferred because they require human validation or a dedicated cycle.
pub fn partition_drainable_backlog_ids(
    backlog_ids: &[String],
    contexts: &HashMap<String, backlog::ExecutionContext>,
) -> (Vec<String>, Vec<BacklogDrainSkip>) {
    let mut drainable = Vec::new();
    let mut skipped = Vec::new();
    for id in backlog_ids {
        let key = id.trim().to_ascii_lowercase();
        let ctx = contexts.get(&key).copied().unwrap_or_default();
        if ctx.loop_undrainable() {
            skipped.push(BacklogDrainSkip {
                id: key,
                reason: if ctx.operator_verify_required {
                    "operator_verify"
                } else {
                    "focused_cycle"
                },
            });
        } else {
            drainable.push(id.clone());
        }
    }
    (drainable, skipped)
}

/// Build an id→execution-context map from active backlog-like tracked work
/// components. First-seen context wins on duplicate ids.
pub fn collect_backlog_execution_contexts(
    components: &[element::Component],
    content: &str,
) -> HashMap<String, backlog::ExecutionContext> {
    let mut contexts = HashMap::new();
    for component in components {
        if !matches!(component.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        for (id, ctx) in backlog::active_item_execution_contexts(component.content(content)) {
            contexts.entry(id.to_ascii_lowercase()).or_insert(ctx);
        }
    }
    contexts
}

/// Active backlog ids, lowercased, that are not drainable by the in-session loop.
pub fn deferred_backlog_ids(content: &str) -> HashSet<String> {
    deferred_backlog_ids_split(content, DrainScope::InSessionLoop).all
}

/// Active backlog ids, lowercased, that are not drainable by the supervisor.
pub fn supervisor_deferred_backlog_ids(content: &str) -> HashSet<String> {
    deferred_backlog_ids_split(content, DrainScope::Supervisor).all
}

/// Deferred backlog ids for a drain scope, split by whether an operator's inline
/// verdict on the queue head can satisfy the deferral (`#opverifyanswered`).
#[derive(Debug, Clone, Default)]
pub struct DeferredBacklogIds {
    /// Every id deferred in this scope.
    pub all: HashSet<String>,
    /// Ids whose deferral an operator verdict cannot satisfy: `[focused-cycle]`
    /// asks for a fresh cycle, which no amount of human confirmation supplies.
    pub operator_verdict_immune: HashSet<String>,
    /// `#queueblocked`: every blocked id (open stays-open blocker or `after=`
    /// prerequisite). Never `⏳`. Ids blocked by a stays-open clause are also
    /// in `all` and `operator_verdict_immune`: an inline verdict on the head
    /// does not close the blocker, so it does not unblock the row. `after=`
    /// edges keep their own gate (`#dagdraingate`) and auto-DAG ordering, so
    /// a head already in progress is never preempted by one (GH #162).
    pub blocked: HashSet<String>,
}

impl DeferredBacklogIds {
    /// Whether `id` is still deferred, given whether its head carries an
    /// operator verdict.
    pub fn defers(&self, id: &str, operator_answered: bool) -> bool {
        if operator_answered {
            self.operator_verdict_immune.contains(id)
        } else {
            self.all.contains(id)
        }
    }
}

/// Build the scoped deferral sets from tracked-work components.
pub fn deferred_backlog_ids_split(content: &str, scope: DrainScope) -> DeferredBacklogIds {
    let mut deferred = DeferredBacklogIds::default();
    let Ok(components) = element::parse(content) else {
        return deferred;
    };
    for comp in &components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        for (id, ctx) in backlog::active_item_execution_contexts(body) {
            let undrainable = match scope {
                DrainScope::InSessionLoop => ctx.loop_undrainable(),
                DrainScope::Supervisor => ctx.supervisor_undrainable(),
            };
            if !undrainable {
                continue;
            }
            let key = id.to_ascii_lowercase();
            let immune = match scope {
                DrainScope::InSessionLoop => ctx.loop_undrainable_with_operator_verdict(true),
                DrainScope::Supervisor => ctx.supervisor_undrainable_with_operator_verdict(true),
            };
            if immune {
                deferred.operator_verdict_immune.insert(key.clone());
            }
            deferred.all.insert(key);
        }
    }
    // `#queueblocked`: a blocked row is undrainable in every scope until its
    // last open blocker closes. Folding it into the deferral set keeps every
    // drain gate, drainable count, and selection projection in agreement.
    let blockers = crate::queue_blocked::queue_blockers(content);
    for id in blockers.deferred_ids() {
        deferred.operator_verdict_immune.insert(id.clone());
        deferred.all.insert(id);
    }
    deferred.blocked = blockers.blocked_ids();
    deferred
}

/// Active backlog ids, lowercased, carrying `[clean-session]`.
pub fn clean_session_backlog_ids(content: &str) -> HashSet<String> {
    execution_context_ids(content, |ctx| ctx.clean_session_required)
}

/// Active backlog ids, lowercased, carrying `[focused-cycle]`.
pub fn focused_cycle_backlog_ids(content: &str) -> HashSet<String> {
    execution_context_ids(content, |ctx| ctx.focused_cycle_required)
}

/// Active backlog ids, lowercased, requiring supervisor context reset.
pub fn context_reset_backlog_ids(content: &str) -> HashSet<String> {
    execution_context_ids(content, |ctx| {
        ctx.clean_session_required || ctx.focused_cycle_required
    })
}

fn execution_context_ids(
    content: &str,
    predicate: impl Fn(&backlog::ExecutionContext) -> bool,
) -> HashSet<String> {
    let mut ids = HashSet::new();
    let Ok(components) = element::parse(content) else {
        return ids;
    };
    for comp in &components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        for (id, ctx) in backlog::active_item_execution_contexts(body) {
            if predicate(&ctx) {
                ids.insert(id.to_ascii_lowercase());
            }
        }
    }
    ids
}

/// Whether queue `head` maps to a `[clean-session]` backlog item.
pub fn head_requires_clean_session_in(content: &str, head: &str) -> bool {
    backlog::item_execution_context(head).clean_session_required
        || head_id_in_set(head, &clean_session_backlog_ids(content))
}

/// Whether queue `head` maps to a `[focused-cycle]` backlog item.
pub fn head_requires_focused_cycle_in(content: &str, head: &str) -> bool {
    backlog::item_execution_context(head).focused_cycle_required
        || head_id_in_set(head, &focused_cycle_backlog_ids(content))
}

/// Whether queue `head` maps to a backlog item requiring supervisor context reset.
pub fn head_requires_context_reset_in(content: &str, head: &str) -> bool {
    let inline = backlog::item_execution_context(head);
    inline.clean_session_required
        || inline.focused_cycle_required
        || head_id_in_set(head, &context_reset_backlog_ids(content))
}

fn head_id_in_set(head: &str, ids: &HashSet<String>) -> bool {
    if ids.is_empty() {
        return false;
    }
    let id = extract_head_id(head)
        .map(|i| i.to_ascii_lowercase())
        .unwrap_or_else(|| head.trim().to_ascii_lowercase());
    ids.contains(&id)
}

/// `#queueoperatoranswer`: lowercased operator-request ids that a live WORK row in the
/// selected queue still carries (`ROW [#subagent-operator-request-x]`, marked
/// `🙋` or not).
pub fn operator_request_ids_with_work_rows(content: &str) -> HashSet<String> {
    let Some((_, entries)) = queue_component_entries(content) else {
        return HashSet::new();
    };
    document_queue::prompts(&entries)
        .into_iter()
        .flat_map(|prompt| {
            agent_doc_document::queue_projection::operator_request_annotation_ids(&prompt.text)
        })
        .collect()
}

/// `#queueoperatoranswer`: which operator answer rows are NOT fresh subagent dispatch
/// work in `content`.
///
/// An answer row (`[#request-id]: <answer>`, or the request id as its own row
/// with the answer nested beneath it) routes to the worker that asked. While a
/// live work row still carries the request, the answer belongs to that row's
/// attempt and the coordinator relays it there. Once the request's todo is
/// closed there is nothing left to route. Only an open request whose work row
/// is gone leaves the answer row itself as the dispatchable carrier.
#[derive(Debug, Default, Clone)]
pub struct AnswerRowRouting {
    work_row_ids: HashSet<String>,
    open_todo_ids: HashSet<String>,
}

impl AnswerRowRouting {
    pub fn for_content(content: &str) -> Self {
        Self {
            work_row_ids: operator_request_ids_with_work_rows(content),
            open_todo_ids: agent_doc_element::todo::open_todo_ids(content),
        }
    }

    /// True when `head` is an operator answer row that must not be dispatched.
    pub fn holds_from_dispatch(&self, head: &str) -> bool {
        agent_doc_document::queue_projection::operator_answer_row_request_id(head)
            .is_some_and(|id| self.work_row_ids.contains(&id) || !self.open_todo_ids.contains(&id))
    }
}

/// Lowercased ids of active queue heads carrying an operator verdict
/// (`#opverifyanswered`).
pub fn operator_answered_head_ids(content: &str) -> HashSet<String> {
    let Some((_, entries)) = queue_component_entries(content) else {
        return HashSet::new();
    };
    document_queue::prompts(&entries)
        .into_iter()
        .filter(|prompt| head_carries_operator_verdict(&prompt.text))
        .filter_map(|prompt| extract_head_id(&prompt.text))
        .map(|id| id.to_ascii_lowercase())
        .collect()
}

/// Lowercased ids of GATED rows in `agent:review`.
///
/// `#qreviewdefer`: `deferred_backlog_ids_split` and
/// `collect_backlog_execution_contexts` both scope themselves to
/// `backlog | icebox | pending`, so an `[operator-verify]` tag on a review row —
/// the normal home for a gated, externally-blocked item — is invisible to the
/// deferral scan. A gated review row IS an external gate by construction, which
/// is what `agent:review` means, so gatedness alone is the signal here and no
/// tag scan is needed.
///
/// Derived from parsed items rather than a line prefix on purpose: a typed gate
/// (`- [/release]`) is exactly as gated as `- [/]`, and a `strip_prefix("- [/]")`
/// witness would miss it and put the head straight back into the silent-stall
/// shape this exists to fix.
pub fn gated_review_ids(content: &str) -> HashSet<String> {
    let Ok(components) = element::parse(content) else {
        return HashSet::new();
    };
    let mut ids = HashSet::new();
    for comp in &components {
        if !element::is_review_component(&comp.name) {
            continue;
        }
        let (_, items, _) = backlog::parse_items(comp.content(content));
        for item in items {
            if item.state != backlog::PendingState::Gated {
                continue;
            }
            let id = backlog::normalize_pending_id(&item.id);
            if !id.is_empty() {
                ids.insert(id);
            }
        }
    }
    ids
}

/// Count active queue prompt heads that are deferred in-session.
///
/// `#qreviewdefer`: a head is deferred when its id is a deferred backlog id OR a
/// GATED `agent:review` row carrying no operator verdict. Without the second
/// clause a queue whose only head waits on a review gate counted ZERO deferred
/// heads, session-check fell through to `NoDrainableWork` — class `ok`,
/// next_action `no_agent_action`, i.e. "fully drained, nothing to do" — and the
/// operator was left staring at a visible queue item they were never told was
/// theirs to clear. Observed 2026-09-20 on `src/haiven-dev/tasks/fpe.md`.
///
/// The gated-review clause lives HERE rather than in `deferred_backlog_ids`
/// because that set also gates drain eligibility: widening it would defer a
/// genuinely drainable backlog item that happens to share an id with a review
/// row. This counter only reports.
///
/// An operator verdict on the head still satisfies the deferral
/// (`#opverifyanswered`) — that head is the operator action, not a wait for one.
pub fn deferred_head_count(content: &str) -> usize {
    let Some((_, entries)) = queue_component_entries(content) else {
        return 0;
    };
    let deferred_ids = deferred_backlog_ids(content);
    let gated_review = gated_review_ids(content);
    let operator_answered = operator_answered_head_ids(content);
    document_queue::prompts(&entries)
        .into_iter()
        .filter_map(|prompt| extract_head_id(&prompt.text))
        .map(|id| id.to_ascii_lowercase())
        .filter(|id| {
            deferred_ids.contains(id)
                || (gated_review.contains(id) && !operator_answered.contains(id))
        })
        .count()
}

/// Lowercased ids of live queue heads whose agent work is finished and which
/// now wait on the operator (`#queueawaitop`).
///
/// A head qualifies when its tracked item is `[operator-verify]` (the deferral
/// no agent drain scope ever runs) or it names a GATED `agent:review` row, and
/// the head itself carries no inline operator verdict (`do [#id]: <text>`,
/// `#opverifyanswered`) — a verdict makes the head the operator's answer,
/// i.e. agent work again. `[focused-cycle]` is NOT awaiting the operator: the
/// supervisor runs it in a fresh context, so it is excluded.
///
/// Preflight projects `⏳` onto these heads unless a worker claim or the inline
/// selection already projects `🚧`. Before this, a subagent that finished,
/// tagged its item `[operator-verify]` and released its claim left the head as
/// a bare `do [#id]` that read as unstarted.
pub fn awaiting_operator_head_ids(content: &str) -> HashSet<String> {
    let Some((_, entries)) = queue_component_entries(content) else {
        return HashSet::new();
    };
    let operator_verify = deferred_backlog_ids_split(content, DrainScope::Supervisor);
    let gated_review = gated_review_ids(content);
    document_queue::prompts(&entries)
        .into_iter()
        .filter(|prompt| !head_carries_operator_verdict(&prompt.text))
        .filter_map(|prompt| extract_head_id(&prompt.text))
        .map(|id| id.to_ascii_lowercase())
        // `#queueblocked`: a blocked head shows `⛔`, never `⏳`. Its agent
        // work cannot start until the blockers close, so it is not waiting on
        // an operator verdict yet.
        .filter(|id| !operator_verify.blocked.contains(id))
        .filter(|id| operator_verify.all.contains(id) || gated_review.contains(id))
        .collect()
}

/// Active queue continuation head, without drainability filtering.
pub fn live_continuation_head(content: &str) -> Option<String> {
    let (_, activation) = active_queue(content)?;
    let head = document_queue::first_prompt(&activation.entries_after)?;
    Some(extract_head_id(&head.text).unwrap_or_else(|| head.text.trim().to_string()))
}

/// First drainable active queue prompt for `scope`.
pub fn drainable_head_prompt_for_scope(content: &str, scope: DrainScope) -> Option<QueuePrompt> {
    drainable_head_prompt_for_scope_with_presets(
        content,
        scope,
        &frontmatter::PromptPresets::default(),
    )
}

/// [`drainable_head_prompt_for_scope`] with the document's effective preset
/// map (GH #381).
pub fn drainable_head_prompt_for_scope_with_presets(
    content: &str,
    scope: DrainScope,
    presets: &frontmatter::PromptPresets,
) -> Option<QueuePrompt> {
    let (queue_facts, activation) =
        active_queue_for_supervisor_start(content, matches!(scope, DrainScope::Supervisor))?;
    eligible_head_prompt_from_entries(
        content,
        &activation.entries_after,
        queue_facts.preset_supplies_directive,
        scope,
        &preset_only_identity_ids_with_presets(content, presets),
    )
}

/// Pending manual work shares drain eligibility without granting auto activation.
/// Stop recovery uses this projection so deferred review mirrors cannot create
/// response debt after closeout. Explicit stop/time fences still park the work.
pub fn pending_head_prompt_text(content: &str, scope: DrainScope) -> Option<String> {
    pending_head_prompt_text_excluding_claimed(content, scope, &ClaimedQueueItems::none())
}

/// [`pending_head_prompt_text`], skipping heads `claimed` by a worker.
///
/// `#fixlazilycodex`: a head a subagent owns (`agent:queue subagents`) is not
/// session-owned prompt debt. Stop recovery that saw such a head after a
/// committed cycle reopened a fresh cycle and captured Codex's closing status
/// chat as its "recovered response" — every Stop, until a status repeated
/// verbatim and the reopen failed closed against HEAD. Claimed heads are
/// filtered out before the stop/time fences and eligibility are evaluated,
/// exactly as [`live_drainable_head_prompt_text_excluding_claimed`] does.
pub fn pending_head_prompt_text_excluding_claimed(
    content: &str,
    scope: DrainScope,
    claimed: &ClaimedQueueItems,
) -> Option<String> {
    pending_head_prompt_text_excluding_claimed_with_presets(
        content,
        scope,
        claimed,
        &frontmatter::PromptPresets::default(),
    )
}

/// [`pending_head_prompt_text_excluding_claimed`] with the document's
/// effective preset map (GH #381).
pub fn pending_head_prompt_text_excluding_claimed_with_presets(
    content: &str,
    scope: DrainScope,
    claimed: &ClaimedQueueItems,
    presets: &frontmatter::PromptPresets,
) -> Option<String> {
    if crate::queue_heads::queue_is_explicitly_stopped(content) {
        return None;
    }
    let (queue_facts, entries) = queue_component_entries_excluding_claimed(content, claimed)?;
    let entries: Vec<QueueEntry> = entries
        .into_iter()
        .filter(|entry| match entry {
            QueueEntry::Prompt(prompt) => !claimed.claims(&prompt.text),
            _ => true,
        })
        .collect();
    if document_queue::has_stop_fence_at_head(&entries)
        || document_queue::time_gate_at_head(&entries).is_some()
    {
        return None;
    }
    let head = eligible_head_prompt_from_entries(
        content,
        &entries,
        queue_facts.preset_supplies_directive,
        scope,
        &preset_only_identity_ids_with_presets(content, presets),
    )?;
    Some(strip_in_progress_marker(&head.text))
}

fn eligible_head_prompt_from_entries(
    content: &str,
    entries: &[QueueEntry],
    preset_supplies_directive: bool,
    scope: DrainScope,
    preset_only: &HashSet<String>,
) -> Option<QueuePrompt> {
    let open_backlog = open_backlog_ids_from_content(content);
    let deferred_ids = deferred_backlog_ids_split(content, scope);
    first_drainable_head(
        entries,
        open_backlog.as_ref(),
        &deferred_ids,
        &after_deps_from_content(content),
        preset_only,
        preset_supplies_directive,
        scope,
    )
    .cloned()
}

/// Live drainable active queue head for `scope`.
pub fn live_drainable_continuation_head(content: &str, scope: DrainScope) -> Option<String> {
    live_drainable_continuation_head_entry(content, scope).map(|entry| entry.head)
}

/// [`live_drainable_continuation_head`] with the document's effective preset
/// map (GH #381).
pub fn live_drainable_continuation_head_with_presets(
    content: &str,
    scope: DrainScope,
    presets: &frontmatter::PromptPresets,
) -> Option<String> {
    live_drainable_continuation_head_entry_with_presets(content, scope, presets)
        .map(|entry| entry.head)
}

/// One drainable queue head under both of its names (GH #319).
///
/// `head` is what [`live_drainable_continuation_head`] returns: the bare
/// tracked id (`beyp`) for an id-backed head, otherwise the prompt text. It is
/// a *routing* name only. `prompt_text` is the marker-stripped line itself
/// (`[#beyp]: approved`), the only form a claim identity may be derived from:
/// [`ClaimedQueueItems::claims`] keys through [`crate::queue_claim::claim_identity`],
/// which reads a bare `beyp` as free text and so never matches the claim made
/// on the line. Every claim check against a drainable head must use
/// `prompt_text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrainableContinuationHead {
    pub head: String,
    pub prompt_text: String,
}

/// [`live_drainable_continuation_head`], also carrying the head's prompt text
/// so a caller can check claims against the line's real identity (GH #319).
pub fn live_drainable_continuation_head_entry(
    content: &str,
    scope: DrainScope,
) -> Option<DrainableContinuationHead> {
    live_drainable_continuation_head_entry_with_presets(
        content,
        scope,
        &frontmatter::PromptPresets::default(),
    )
}

/// [`live_drainable_continuation_head_entry`] with the document's effective
/// preset map (GH #381).
pub fn live_drainable_continuation_head_entry_with_presets(
    content: &str,
    scope: DrainScope,
    presets: &frontmatter::PromptPresets,
) -> Option<DrainableContinuationHead> {
    let head = drainable_head_prompt_for_scope_with_presets(content, scope, presets)?;
    let prompt_text = strip_in_progress_marker(&head.text);
    let preset_only = preset_only_identity_ids_with_presets(content, presets);
    let head = tracked_head_id(&prompt_text, &preset_only).unwrap_or_else(|| prompt_text.clone());
    Some(DrainableContinuationHead { head, prompt_text })
}

/// [`live_drainable_continuation_head`], skipping heads `claimed` by a worker
/// (`#queueclaim`, `#steeringwake`). The idle supervisor must never wake the
/// session for a head a subagent (or the parent's own deferred claim) already
/// owns: that is a double dispatch of in-flight work.
pub fn live_drainable_continuation_head_excluding_claimed(
    content: &str,
    scope: DrainScope,
    claimed: &ClaimedQueueItems,
) -> Option<String> {
    live_drainable_continuation_head_excluding_claimed_with_presets(
        content,
        scope,
        claimed,
        &frontmatter::PromptPresets::default(),
    )
}

/// [`live_drainable_continuation_head_excluding_claimed`] with the document's
/// effective preset map (GH #381): the supervisor idle watch's wake decision
/// must treat a project-preset head exactly as preflight does.
pub fn live_drainable_continuation_head_excluding_claimed_with_presets(
    content: &str,
    scope: DrainScope,
    claimed: &ClaimedQueueItems,
    presets: &frontmatter::PromptPresets,
) -> Option<String> {
    live_drainable_continuation_head_for_owner(content, scope, claimed, claimed, presets)
}

/// [`live_drainable_continuation_head_excluding_claimed_with_presets`] that
/// selects with `selection` and suppresses `suppressed` inside that queue.
pub fn live_drainable_continuation_head_for_owner(
    content: &str,
    scope: DrainScope,
    selection: &ClaimedQueueItems,
    suppressed: &ClaimedQueueItems,
    presets: &frontmatter::PromptPresets,
) -> Option<String> {
    let stripped =
        live_drainable_head_prompt_text_for_owner(content, scope, selection, suppressed, presets)?;
    let preset_only = preset_only_identity_ids_with_presets(content, presets);
    Some(tracked_head_id(&stripped, &preset_only).unwrap_or(stripped))
}

/// [`live_drainable_continuation_head_excluding_claimed`], returning the head's
/// marker-stripped **prompt text** (`do [#id]`) instead of its bare id.
///
/// `#regressionawaitingoperat`: preflight synthesizes this head as the cycle's
/// continuation diff. Its `queue_supervisor_drainable` gate already asked
/// "is SOME unclaimed head drainable?", but the head it then synthesized was
/// the first unclaimed head regardless of drainability, so an `[operator-verify]`
/// head (`⏳`) above a drainable one was reported every preflight as a fresh
/// `[user+] do [#id]` prompt and became the cycle's turn driver. The gate and
/// the head must be the same derivation, which this is.
pub fn live_drainable_head_prompt_text_excluding_claimed(
    content: &str,
    scope: DrainScope,
    claimed: &ClaimedQueueItems,
) -> Option<String> {
    live_drainable_head_prompt_text_excluding_claimed_with_presets(
        content,
        scope,
        claimed,
        &frontmatter::PromptPresets::default(),
    )
}

/// [`live_drainable_head_prompt_text_excluding_claimed`] with the document's
/// effective preset map (GH #381).
pub fn live_drainable_head_prompt_text_excluding_claimed_with_presets(
    content: &str,
    scope: DrainScope,
    claimed: &ClaimedQueueItems,
    presets: &frontmatter::PromptPresets,
) -> Option<String> {
    live_drainable_head_prompt_text_for_owner(content, scope, claimed, claimed, presets)
}

/// Drainable head of the queue `selection` chooses, ignoring `suppressed`
/// heads inside that queue without selecting a later one.
///
/// `selection` only chooses the queue. A `Reported` attempt is in that set so
/// an earlier queue with nothing left to dispatch does not hide a later run,
/// and it stays drainable when its own queue is the one selected
/// (`#reportedwritebackdebt`). `suppressed` is what drops a head: in-flight
/// claims, this cycle's dispatch, and holds. Passing dispatch heads as
/// `selection` would pause the queue just chosen and name the next `agent:run`.
pub fn live_drainable_head_prompt_text_for_owner(
    content: &str,
    scope: DrainScope,
    selection: &ClaimedQueueItems,
    suppressed: &ClaimedQueueItems,
    presets: &frontmatter::PromptPresets,
) -> Option<String> {
    let (queue_facts, activation) = active_queue_for_supervisor_start_excluding_claimed(
        content,
        matches!(scope, DrainScope::Supervisor),
        selection,
    )?;
    let entries: Vec<QueueEntry> = activation
        .entries_after
        .into_iter()
        .filter(|entry| match entry {
            QueueEntry::Prompt(prompt) => !suppressed.claims(&prompt.text),
            _ => true,
        })
        .collect();
    let head = eligible_head_prompt_from_entries(
        content,
        &entries,
        queue_facts.preset_supplies_directive,
        scope,
        &preset_only_identity_ids_with_presets(content, presets),
    )?;
    Some(strip_in_progress_marker(&head.text))
}

/// Live drainable active queue head **prompt text** for `scope` (`do [#id]`),
/// where [`live_drainable_continuation_head`] returns only the bare id.
///
/// `#planhead`: this is the single derivation of "which head is next" for any
/// caller that needs the dispatchable prompt. `queue_heads::active_queue_prompt`
/// returns the first head regardless of drainability, so a second caller using it
/// can name an `[operator-verify]` head that preflight already deferred.
pub fn live_drainable_head_prompt_text(content: &str, scope: DrainScope) -> Option<String> {
    live_drainable_head_prompt_text_with_presets(
        content,
        scope,
        &frontmatter::PromptPresets::default(),
    )
}

/// [`live_drainable_head_prompt_text`] with the document's effective preset
/// map (GH #381).
pub fn live_drainable_head_prompt_text_with_presets(
    content: &str,
    scope: DrainScope,
    presets: &frontmatter::PromptPresets,
) -> Option<String> {
    let head = drainable_head_prompt_for_scope_with_presets(content, scope, presets)?;
    Some(strip_in_progress_marker(&head.text))
}

/// `#focusedfallback`: the head only the supervisor scope drains (a
/// `[focused-cycle]` head), when the in-session loop has none of its own.
///
/// The in-session loop yields such a head to the supervisor's
/// clear-and-continue path. When that supervisor cannot drain (none live, a
/// stale heartbeat, or a stale binary), the caller hands this head back to the
/// in-session loop instead of stalling the queue on the operator.
pub fn supervisor_only_head_prompt_text(content: &str) -> Option<String> {
    supervisor_only_head_prompt_text_with_presets(content, &frontmatter::PromptPresets::default())
}

/// [`supervisor_only_head_prompt_text`] with the document's effective preset
/// map (GH #381).
pub fn supervisor_only_head_prompt_text_with_presets(
    content: &str,
    presets: &frontmatter::PromptPresets,
) -> Option<String> {
    if live_drainable_head_prompt_text_with_presets(content, DrainScope::InSessionLoop, presets)
        .is_some()
    {
        return None;
    }
    live_drainable_head_prompt_text_with_presets(content, DrainScope::Supervisor, presets)
}

/// Guidance printed when the in-session loop takes over a `[focused-cycle]` head
/// because its supervisor cannot drain it (`#focusedfallback`).
pub fn supervisor_unavailable_fallback_guidance(readiness: &str) -> String {
    format!(
        "queue continuation required — the next head is `[focused-cycle]`, which normally yields to the supervisor's clear-and-continue path, but the supervisor cannot drain it (supervisor_drain_readiness={readiness}). Drain it IN-SESSION now instead of handing it back: a stale or missing supervisor is not a stop reason, and asking the operator to re-trigger is a stall (#focusedfallback)."
    )
}

/// The head prompt a dispatching caller should work, honouring the same deferral
/// rules preflight applies when it fills `selected_queue_prompts`.
///
/// `#planhead`: `agent-doc plan` used to call
/// [`crate::queue_heads::active_queue_prompt`] directly, which returns the first
/// head regardless of drainability. On a go-mode queue whose head is
/// `[operator-verify]`, plan therefore named a human-gated item as the execution
/// contract while preflight had already deferred it — and SKILL.md step 0d tells
/// the agent to treat plan's output as authoritative.
///
/// Prefers the drain-active head. A queue that is active but NOT in explicit
/// go-mode has no auto-drain, so its head is a manual prompt rather than a drain
/// item; that case falls back to the plain active head, still filtered by the
/// scope's deferral set so an `[operator-verify]` item is never selected either
/// way.
pub fn dispatchable_head_prompt_text(content: &str, scope: DrainScope) -> Option<String> {
    dispatchable_head_prompt_text_excluding_claimed(content, scope, &ClaimedQueueItems::none())
}

/// [`dispatchable_head_prompt_text`], skipping heads `claimed` by a worker.
///
/// Connected planning uses this projection so queue-level subagent heads do not
/// leak back into the parent session as inline work. The manual-queue fallback
/// remains part of this selector: unlike an explicit go-mode queue, a manually
/// activated queue has no live drainable head but still supplies plan work.
pub fn dispatchable_head_prompt_text_excluding_claimed(
    content: &str,
    scope: DrainScope,
    claimed: &ClaimedQueueItems,
) -> Option<String> {
    dispatchable_head_prompt_text_excluding_claimed_with_presets(
        content,
        scope,
        claimed,
        &frontmatter::PromptPresets::default(),
    )
}

/// [`dispatchable_head_prompt_text_excluding_claimed`] with the document's
/// effective preset map (GH #381).
pub fn dispatchable_head_prompt_text_excluding_claimed_with_presets(
    content: &str,
    scope: DrainScope,
    claimed: &ClaimedQueueItems,
    presets: &frontmatter::PromptPresets,
) -> Option<String> {
    if let Some(text) = live_drainable_head_prompt_text_excluding_claimed_with_presets(
        content, scope, claimed, presets,
    ) {
        return Some(text);
    }
    let head = crate::queue_heads::active_queue_prompt(content)?;
    if claimed.claims(&head) {
        return None;
    }
    let deferred = deferred_backlog_ids_split(content, scope);
    match extract_head_id(&head) {
        Some(id) if deferred.defers(&id, false) => None,
        _ => Some(head),
    }
}

/// [`dispatchable_head_prompt_text_excluding_claimed_with_presets`] that keeps
/// the manual-queue fallback on the owner-selected queue.
///
/// Suppressing that queue's dispatch heads returns no inline prompt. It does
/// not fall through to `selected()`, which would name a yet-later `agent:run`
/// in the same plan.
pub fn dispatchable_head_prompt_text_for_owner(
    content: &str,
    scope: DrainScope,
    selection: &ClaimedQueueItems,
    suppressed: &ClaimedQueueItems,
    presets: &frontmatter::PromptPresets,
) -> Option<String> {
    if let Some(text) =
        live_drainable_head_prompt_text_for_owner(content, scope, selection, suppressed, presets)
    {
        return Some(text);
    }
    let components = element::parse(content).ok()?;
    let queues = crate::queue_set::QueueSet::parse(content, &components).ok()?;
    let control = document_queue_control(content);
    let block = queues
        .selected_reaching_inline_work_with_control(selection, control.as_deref())
        .or_else(|| queues.selected())?;
    let entries = document_queue::parse(block.component.content(content)).ok()?;
    let entries = crate::nested_claim_steer::expand_if_installed(entries);
    let has_auto = document_queue::has_auto_attr(&block.component.attrs);
    let (fm, _) = frontmatter::parse(content).ok()?;
    let activation = document_queue::resolve_activation(
        &entries,
        has_auto,
        false,
        fm.queue_active.unwrap_or(false),
    );
    if !activation.active {
        return None;
    }
    let head = document_queue::prompts(&activation.entries_after)
        .into_iter()
        .map(|prompt| strip_in_progress_marker(&prompt.text))
        .find(|text| !suppressed.claims(text))?;
    let deferred = deferred_backlog_ids_split(content, scope);
    match extract_head_id(&head) {
        Some(id) if deferred.defers(&id, false) => None,
        _ => Some(head),
    }
}

/// Count agent-drainable heads in the active queue for the in-session loop.
pub fn drainable_head_count(content: &str) -> usize {
    drainable_head_count_excluding_claimed(content, &ClaimedQueueItems::none())
}

/// [`drainable_head_count`], not counting heads `claimed` by a worker outside
/// the in-session loop (`#queueclaim`).
pub fn drainable_head_count_excluding_claimed(content: &str, claimed: &ClaimedQueueItems) -> usize {
    drainable_head_count_excluding_claimed_with_presets(
        content,
        claimed,
        &frontmatter::PromptPresets::default(),
    )
}

/// [`drainable_head_count_excluding_claimed`] with the document's effective
/// preset map (GH #381). A head whose leading `#name` is a preset inherited
/// from `.agent-doc/project.md`, and not also open tracked work, counts as a
/// drainable preset invocation without a backlog item.
pub fn drainable_head_count_excluding_claimed_with_presets(
    content: &str,
    claimed: &ClaimedQueueItems,
    presets: &frontmatter::PromptPresets,
) -> usize {
    drainable_head_count_for_owner(content, claimed, claimed, presets)
}

/// [`drainable_head_count_excluding_claimed_with_presets`] for one owner queue.
///
/// `selection` chooses the queue. `suppressed` drops heads inside it (this
/// cycle's dispatch and holds) without moving the count onto a later queue.
/// Heads present only in `selection`, including a `Reported` writeback, still
/// count when that queue is the one selected.
pub fn drainable_head_count_for_owner(
    content: &str,
    selection: &ClaimedQueueItems,
    suppressed: &ClaimedQueueItems,
    presets: &frontmatter::PromptPresets,
) -> usize {
    let Some((queue_facts, activation)) =
        active_queue_for_supervisor_start_excluding_claimed(content, false, selection)
    else {
        return 0;
    };
    let open_backlog = open_backlog_ids_from_content(content);
    let deferred_ids = deferred_backlog_ids_split(content, DrainScope::InSessionLoop);
    let after_deps = after_deps_from_content(content);
    let preset_only = preset_only_identity_ids_with_presets(content, presets);
    document_queue::prompts(&activation.entries_after)
        .into_iter()
        .filter(|prompt| !suppressed.claims(&prompt.text))
        .filter(|prompt| {
            head_is_drainable(
                &prompt.text,
                open_backlog.as_ref(),
                &deferred_ids,
                &after_deps,
                &preset_only,
                queue_facts.preset_supplies_directive,
                DrainScope::InSessionLoop,
            )
        })
        .count()
}

/// Live queue heads that are claimed by a worker outside the in-session loop
/// (`#queueclaim`). Non-zero with no drainable head is the "waiting on
/// in-flight work" state: the loop must end its turn quietly, not re-enter.
pub fn claimed_head_count(content: &str, claimed: &ClaimedQueueItems) -> usize {
    if claimed.is_empty() {
        return 0;
    }
    let Some((_, entries)) = queue_component_entries(content) else {
        return 0;
    };
    document_queue::prompts(&entries)
        .into_iter()
        .filter(|prompt| claimed.claims(&prompt.text))
        .count()
}

/// Identities of every live (unstruck, executable) row of EVERY queue
/// component, the claim-closure set: a claim whose identity is absent here
/// belongs to an item that has closed (`#queueclaim`). `None` when the
/// document has no parseable queue graph, in which case closure cannot be
/// judged and only expiry applies. (It is not `None` merely because no queue
/// is ready: idle, blocked and in-flight queues still list their live rows.)
///
/// A claim is judged against its own queue (`#claimclosureallqueues`), never
/// against the queue the scheduler happens to select: a claim's queue is the
/// queue where its identity is a live row
/// ([`crate::queue_claim::QueueClaimLedger::follow_edits_in_view`]), so its
/// row is live exactly when its identity is a live row of some queue. Reading
/// only the selected queue judged a claim on a live `agent:loop` row closed
/// while `agent:run` had work, and prune / `active()` / `claimed_items`
/// dropped it, re-offering the claimed row. A struck row is in no queue's
/// live set, so its claim still closes (the strike policy then applies).
/// Drain and dispatch callers that want only the selected queue read
/// [`live_queue_head_texts`] / [`live_queue_head_rows`] instead.
pub fn live_queue_head_identities(
    content: &str,
) -> Option<HashSet<agent_doc_element_queue::QueueItemIdentity>> {
    let components = element::parse(content).ok()?;
    let queue_set = crate::queue_set::QueueSet::parse(content, &components).ok()?;
    Some(
        queue_set
            .blocks()
            .iter()
            .flat_map(|block| {
                let entries = crate::nested_claim_steer::expand_slice_if_installed(&block.entries);
                document_queue::prompts(entries.as_ref())
                    .into_iter()
                    .map(|prompt| crate::queue_claim::claim_identity(&prompt.text))
                    .collect::<Vec<_>>()
            })
            .collect(),
    )
}

/// Texts of the live queue heads, in queue order (`None` when no queue parses).
pub fn live_queue_head_texts(content: &str) -> Option<Vec<String>> {
    let (_, entries) = queue_component_entries(content)?;
    Some(
        document_queue::prompts(&entries)
            .into_iter()
            // Lifecycle glyphs are a binary-owned projection, never part of
            // the operator-authored head stored in a claim or its anchor.
            .map(|prompt| strip_in_progress_marker(prompt.text.trim()))
            .collect(),
    )
}

/// Live queue-head rows in queue order, each still carrying its binary-owned
/// lifecycle and `[#owner#generation]` projection (`None` when no queue
/// parses). The claim ledger's edit follow reads the projection as row
/// evidence, so a claim stays on the row it was projected onto even when a
/// text-identical sibling exists (`#claimrowmigrate`). Callers comparing work
/// text use [`live_queue_head_texts`].
pub fn live_queue_head_rows(content: &str) -> Option<Vec<String>> {
    let (_, entries) = queue_component_entries(content)?;
    Some(
        document_queue::prompts(&entries)
            .into_iter()
            .map(|prompt| prompt.text.trim().to_string())
            .collect(),
    )
}

/// Count active queue entries that are predicate-proven non-drainable noise.
pub fn queue_stale_noise_lines(content: &str) -> usize {
    let Some((queue_facts, entries)) = queue_component_entries(content) else {
        return 0;
    };
    entries
        .iter()
        .map(|entry| match entry {
            QueueEntry::Prompt(prompt)
                if is_noise_queue_head(&prompt.text, queue_facts.preset_supplies_directive) =>
            {
                // `#queuecruft`: a bulleted item owns its continuation lines,
                // so a noise item counts every source line it spans.
                if prompt.multiline {
                    1
                } else {
                    prompt.text.lines().count().max(1)
                }
            }
            QueueEntry::Freeform(line) => usize::from(document_queue::is_noise_freeform_line(line)),
            _ => 0,
        })
        .sum()
}

#[derive(Debug, Clone)]
struct QueueFacts {
    has_auto: bool,
    /// The marker's attributes, read through `control_binding` so the marker
    /// and frontmatter controls have one precedence (`#qbindingone`).
    attrs: HashMap<String, String>,
    preset_supplies_directive: bool,
}

/// Frontmatter `queue:` value the control binding reads, when one is present.
pub fn document_queue_control(content: &str) -> Option<String> {
    let (fm, _) = frontmatter::parse(content).ok()?;
    crate::control_binding::frontmatter_queue_control(&fm).map(str::to_string)
}

fn queue_component_entries(content: &str) -> Option<(QueueFacts, Vec<QueueEntry>)> {
    queue_component_entries_excluding_claimed(content, &ClaimedQueueItems::none())
}

fn queue_component_entries_excluding_claimed(
    content: &str,
    claimed: &ClaimedQueueItems,
) -> Option<(QueueFacts, Vec<QueueEntry>)> {
    queue_component_entries_from(content, claimed, false)
}

/// Drain and supervisor selection. A fully delegated queue yields to a later
/// queue that has inline work. [`queue_component_entries_excluding_claimed`]
/// does not: `queue brief` and claim identity still read the dispatch owner.
fn queue_component_entries_reaching_inline(
    content: &str,
    claimed: &ClaimedQueueItems,
) -> Option<(QueueFacts, Vec<QueueEntry>)> {
    queue_component_entries_from(content, claimed, true)
}

fn queue_component_entries_from(
    content: &str,
    claimed: &ClaimedQueueItems,
    reaching_inline: bool,
) -> Option<(QueueFacts, Vec<QueueEntry>)> {
    let components = element::parse(content).ok()?;
    let queue_set = crate::queue_set::QueueSet::parse(content, &components).ok()?;
    let control = document_queue_control(content);
    let selected = if reaching_inline {
        queue_set.selected_reaching_inline_work_with_control(claimed, control.as_deref())
    } else {
        queue_set.selected_excluding_claimed_with_control(claimed, control.as_deref())
    };
    let queue_component = selected
        // Preserve the selected queue's blocked/claimed facts when no sibling
        // is available to this drain owner.
        .or_else(|| queue_set.selected())?
        .component;
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = document_queue::parse(body).ok()?;
    let entries = crate::nested_claim_steer::expand_if_installed(entries);
    Some((
        QueueFacts {
            has_auto: document_queue::has_auto_attr(&queue_component.attrs),
            attrs: queue_component.attrs.clone(),
            preset_supplies_directive: crate::prompt_component_attrs::component_preset(
                &queue_component.attrs,
            )
            .is_some(),
        },
        entries,
    ))
}

fn active_queue(content: &str) -> Option<(QueueFacts, document_queue::QueueActivation)> {
    active_queue_for_supervisor_start_excluding_claimed(content, false, &ClaimedQueueItems::none())
}

fn active_queue_for_supervisor_start(
    content: &str,
    allow_supervisor_start: bool,
) -> Option<(QueueFacts, document_queue::QueueActivation)> {
    active_queue_for_supervisor_start_excluding_claimed(
        content,
        allow_supervisor_start,
        &ClaimedQueueItems::none(),
    )
}

fn active_queue_for_supervisor_start_excluding_claimed(
    content: &str,
    allow_supervisor_start: bool,
    claimed: &ClaimedQueueItems,
) -> Option<(QueueFacts, document_queue::QueueActivation)> {
    let (fm, _) = frontmatter::parse(content).ok()?;
    // `#qstartinert`: `queue_active` is the LEGACY activation flag. `control_binding`
    // made `queue:` (and its marker spelling) canonical and writes only that, so
    // requiring a persisted `queue_active: true` here made drainability depend on a
    // field the current writers no longer emit: a document could carry an explicit
    // `go`/`start` control, activate, mirror its entries in, and still report
    // `drainable_head_count: 0` forever. The explicit-control gate below is the real
    // authority; `queue_active` now only participates as an explicit halt.
    //
    // `queue_active: false` IS meaningful — the drain/clear path writes it when a
    // queue finishes — so with no marker control it still stops drainability, as
    // does `queue: stop`.
    //
    // `#qbindingone` (GH #79): the gate is the resolved control binding, the same
    // authority `activation` uses, never a second reading of raw frontmatter.
    // Checking `queue: stop` here first made a marker `go` beside a stale
    // `queue: stop` ACTIVE for preflight yet `drainable_head_count: 0` here, so
    // the queue never drained. The binding lets an explicit marker token win
    // (`#qactsync`), keeps a lone legacy `queue_active` flag inert, and applies
    // the `#queuegodefault` go when neither surface carries a control.
    //
    // `#queuegodefaultdrain`: that default `go` drains here too. A drain clears
    // the control (`#queuestopretire`), so excluding control-less queues left
    // every finished queue undispatched by the idle watch and the Codex Stop
    // hook once a new head was added; only an in-session invocation ran it. An
    // operator who wants a queue kept by hand writes `pause` or `stop`.
    let (queue_facts, entries) = queue_component_entries_reaching_inline(content, claimed)?;
    let control = crate::control_binding::frontmatter_queue_control(&fm);
    let explicit_go = crate::control_binding::explicit_queue_go_mode(&queue_facts.attrs, control);
    let explicit_start = allow_supervisor_start
        && crate::control_binding::explicit_queue_start_mode(&queue_facts.attrs, control);
    if !explicit_go && !explicit_start {
        return None;
    }
    let activation =
        document_queue::resolve_activation(&entries, queue_facts.has_auto, false, true);
    if !activation.active
        || document_queue::has_stop_fence_at_head(&activation.entries_after)
        || document_queue::time_gate_at_head(&activation.entries_after).is_some()
    {
        return None;
    }
    Some((queue_facts, activation))
}

fn first_drainable_head<'a>(
    entries_after: &'a [QueueEntry],
    open_backlog_ids: Option<&HashSet<String>>,
    deferred_ids: &DeferredBacklogIds,
    after_deps: &HashMap<String, Vec<String>>,
    preset_only_ids: &HashSet<String>,
    preset_supplies_directive: bool,
    scope: DrainScope,
) -> Option<&'a QueuePrompt> {
    document_queue::prompts(entries_after)
        .into_iter()
        .find(|prompt| {
            head_is_drainable(
                &prompt.text,
                open_backlog_ids,
                deferred_ids,
                after_deps,
                preset_only_ids,
                preset_supplies_directive,
                scope,
            )
        })
}

fn head_is_drainable(
    text: &str,
    open_backlog_ids: Option<&HashSet<String>>,
    deferred_ids: &DeferredBacklogIds,
    after_deps: &HashMap<String, Vec<String>>,
    preset_only_ids: &HashSet<String>,
    preset_supplies_directive: bool,
    scope: DrainScope,
) -> bool {
    // `🙋 [#id]` is an orthogonal local projection of an open operator todo.
    // The row remains visible in place, but no agent drain scope may dispatch
    // it until completing that same-id todo clears the annotation.
    if !agent_doc_document::queue_projection::operator_action_ids(text).is_empty() {
        return false;
    }
    let drainable = if preset_supplies_directive {
        is_drainable_queue_head_with_context(text, true)
    } else {
        is_drainable_queue_head(text)
    };
    if !drainable {
        return false;
    }
    // `#opverifyanswered`: an `[operator-verify]` head the operator has answered
    // inline (`do [#id]: verified. looks good`) has the human input it was
    // waiting for, so the gate is met and the head drains.
    let operator_answered = head_carries_operator_verdict(text);
    // `#ftimmediate`: free-text heads have no backlog id through which to
    // inherit execution context. Parse the same inline tags directly from the
    // queue text so `[focused-cycle]` yields the in-session loop but remains
    // supervisor-drainable after a forced clear, while `[operator-verify]`
    // stays deferred in both scopes. This is intentionally a projection of the
    // existing tag vocabulary, not a synthesized tracked-work record.
    let inline_context = backlog::item_execution_context(text);
    let inline_undrainable = match scope {
        DrainScope::InSessionLoop => {
            inline_context.loop_undrainable_with_operator_verdict(operator_answered)
        }
        DrainScope::Supervisor => {
            inline_context.supervisor_undrainable_with_operator_verdict(operator_answered)
        }
    };
    if inline_undrainable {
        return false;
    }
    match tracked_head_id(text, preset_only_ids) {
        Some(id) => {
            let norm = id.to_ascii_lowercase();
            if deferred_ids.defers(&norm, operator_answered) {
                return false;
            }
            match open_backlog_ids {
                Some(open) => {
                    if !open.contains(&norm) {
                        // `#qgateverdictdrain`: an inline operator verdict is
                        // fresh executable authority even when its id currently
                        // lives in `agent:review` rather than the open backlog.
                        // Treating every id absent from backlog as a stale mirror
                        // preserved `complete [#id]: ...` during maintenance but
                        // then reported zero drainable heads, so preflight returned
                        // `no_changes` without ever reading the gate-lifting task.
                        // Bare `do [#id]` mirrors still require an open backlog id;
                        // the nonempty annotation is the proof this is not one.
                        return operator_answered;
                    }
                    // `#dagdraingate`: a head with an UNMET declared prerequisite
                    // is not drainable. Without this, `after=` was enforced only
                    // by queue ORDER — and order is not enforcement: head
                    // selection takes the first *drainable* prompt, so a deferred
                    // or skipped prerequisite simply got stepped over and its
                    // dependent ran first. A prerequisite counts as met once it
                    // is no longer an open backlog item (i.e. it was completed).
                    !after_deps.get(&norm).is_some_and(|deps| {
                        deps.iter().any(|dep| {
                            let dep = dep.trim().trim_start_matches('#').to_ascii_lowercase();
                            dep != norm && open.contains(&dep)
                        })
                    })
                }
                // Without a backlog component we cannot tell whether a
                // prerequisite is outstanding; fail open rather than stalling.
                None => true,
            }
        }
        None => true,
    }
}

/// `#presetargdedup`: normalized `#id`s whose ONLY active meaning in the
/// document is a prompt preset -- no backlog/review/icebox item carries them.
///
/// `#subagentintent`: the built-in subagent-intent tags (`#subagent`,
/// `#subagents`, ...) are preset-only too unless a tracked item really carries
/// that id. They mean "dispatch to a subagent" whether or not the document
/// registers them, and reading `#subagent: <url>` as a reference to a missing
/// backlog item made every such head non-drainable.
pub fn preset_only_identity_ids(content: &str) -> HashSet<String> {
    preset_only_identity_ids_with_presets(content, &frontmatter::PromptPresets::default())
}

/// [`preset_only_identity_ids`] with a caller-resolved preset map.
///
/// GH #381: `presets` is the document's EFFECTIVE preset map (the session
/// document's own `presets` overlaid on every `.agent-doc/project.md` layer,
/// `effective_prompt_presets_for_file`). The document-only registry above sees
/// just the session document's frontmatter, so a head naming a project preset
/// (`- #review` with `'#review'` declared only in `.agent-doc/project.md`) was
/// read as tracked id `review`: with an `agent:backlog` lacking it,
/// `head_is_drainable` called it a stale mirror and preflight reported
/// `queue_drainable_head_count: 0`. Inherited presets are never copied into the
/// document (`specs/project-preset-inheritance.md`), so the drain check must
/// take them from the caller. A name an open backlog/review/icebox item also
/// carries stays tracked work, exactly as for a document-local preset.
pub fn preset_only_identity_ids_with_presets(
    content: &str,
    presets: &frontmatter::PromptPresets,
) -> HashSet<String> {
    let identities = backlog::document_active_identities(content);
    let tracked = |id: &str| {
        identities
            .get(id)
            .is_some_and(|sources| sources.iter().any(|source| source != "prompt_presets"))
    };
    let mut ids: HashSet<String> = identities
        .iter()
        .filter(|(_, sources)| sources.iter().all(|source| source == "prompt_presets"))
        .map(|(id, _)| id.clone())
        .collect();
    for key in presets.keys() {
        let id = backlog::normalize_pending_id(key);
        if !id.is_empty() && !tracked(&id) {
            ids.insert(id);
        }
    }
    for tag in crate::subagent_intent::SUBAGENT_INTENT_TAGS {
        if !tracked(tag) {
            ids.insert(tag.to_string());
        }
    }
    ids
}

/// `#presetargdedup`: the tracked-item id a head references, or `None` when
/// its leading `#word` is a prompt-preset invocation (`#gh-fix <url>`). Such a
/// head is free-text work the preset expands; judging it as a mirror of a
/// missing backlog item made every sibling invocation non-drainable, so the
/// loop reported `drainable_head_count: 0` over operator-queued work.
/// GH #381: true when the head's LEADING token is a preset in `preset_only_ids`
/// (`#review`, `#gh-fix <url>`, `do [#review]`, a marker-decorated copy).
///
/// The preflight Todo-backlink audit uses this to exempt preset invocations: a
/// preset names reusable work, not a tracked item, so it never owes a backing
/// backlog item or a Todo backlink. Only the leading token counts, so prose that
/// merely mentions a preset later in the line is still audited as free text.
pub fn head_leads_with_preset(text: &str, preset_only_ids: &HashSet<String>) -> bool {
    let visible = agent_doc_document::queue_projection::strip_priority_markers(
        &strip_in_progress_marker(text),
    );
    let trimmed = visible.trim();
    let topic = trimmed
        .get(..3)
        .filter(|prefix| prefix.eq_ignore_ascii_case("do "))
        .map_or(trimmed, |_| trimmed[3..].trim_start());
    let Some(rest) = topic.strip_prefix("[#").or_else(|| topic.strip_prefix('#')) else {
        return false;
    };
    let name: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect();
    !name.is_empty() && preset_only_ids.contains(&name.to_ascii_lowercase())
}

fn tracked_head_id(text: &str, preset_only_ids: &HashSet<String>) -> Option<String> {
    extract_head_id(text).filter(|id| !preset_only_ids.contains(&id.to_ascii_lowercase()))
}

/// `#dagdraingate`: declared `after=` edges, keyed by dependent id.
fn after_deps_from_content(content: &str) -> HashMap<String, Vec<String>> {
    let Ok(components) = element::parse(content) else {
        return HashMap::new();
    };
    crate::backlog_sync::collect_after_deps(&components, content)
        .into_iter()
        .map(|(k, v)| (k.trim().trim_start_matches('#').to_ascii_lowercase(), v))
        .collect()
}

fn open_backlog_ids_from_content(content: &str) -> Option<HashSet<String>> {
    let components = element::parse(content).ok()?;
    let mut found_backlog = false;
    let mut ids = HashSet::new();
    for comp in &components {
        if !element::is_backlog_component(&comp.name) {
            continue;
        }
        found_backlog = true;
        let body = &content[comp.open_end..comp.close_start];
        let (_, items, _) = backlog::parse_items(body);
        for item in items {
            if !item.is_done() && !item.id.is_empty() {
                ids.insert(item.id.to_ascii_lowercase());
            }
        }
    }
    found_backlog.then_some(ids)
}

pub fn open_review_item_count(content: &str) -> usize {
    let Ok(components) = element::parse(content) else {
        return 0;
    };
    components
        .iter()
        .find(|comp| element::is_review_component(&comp.name))
        .map(|comp| {
            let body = &content[comp.open_end..comp.close_start];
            let (_, items, _) = backlog::parse_items(body);
            items.into_iter().filter(|item| !item.is_done()).count()
        })
        .unwrap_or(0)
}

/// Whether current added at least one open review item relative to prior.
pub fn review_phase_routed(prior: &str, current: &str) -> bool {
    open_review_item_count(current) > open_review_item_count(prior)
}

const QUEUE_DIRECTIVE_VERBS: &[&str] = &[
    "do",
    "fix",
    "run",
    "build",
    "install",
    "commit",
    "push",
    "implement",
    "add",
    "update",
    "investigate",
    "create",
    "make",
    "remove",
    "delete",
    "refactor",
    "review",
    "explain",
    "drive",
    "resume",
    "continue",
    "check",
    "verify",
    "write",
    "test",
    "debug",
    "diagnose",
    "merge",
    "publish",
    "release",
    "bump",
    "rebase",
    "revert",
    "rename",
    "move",
    "split",
    "extract",
    "wire",
    "land",
    "ship",
    "draft",
    "summarize",
    "answer",
    "respond",
    "apply",
    "enable",
    "disable",
    "gate",
    "deploy",
];

const RECURRING_IMPERATIVE_COMMAND_VERBS: &[&str] = &[
    "deploy", "commit", "push", "build", "install", "release", "test", "sync", "recycle",
    "publish", "tag", "bump",
];

/// True when a queue head is a recurring imperative command.
pub fn is_recurring_imperative_head(text: &str) -> bool {
    let normalized = normalize_queue_head_text(text);
    if normalized.is_empty() {
        return false;
    }
    if let Some(id) = extract_head_id(&normalized) {
        let segments: Vec<&str> = id.split(['-', '_']).filter(|s| !s.is_empty()).collect();
        let verb_segments = segments
            .iter()
            .filter(|segment| {
                RECURRING_IMPERATIVE_COMMAND_VERBS.contains(&segment.to_ascii_lowercase().as_str())
            })
            .count();
        if verb_segments >= 2 {
            return true;
        }
    }
    let words: Vec<String> = normalized
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_lowercase())
        .collect();
    if words.is_empty() || words.len() > 3 {
        return false;
    }
    RECURRING_IMPERATIVE_COMMAND_VERBS.contains(&words[0].as_str())
}

fn normalize_queue_head_text(text: &str) -> String {
    let mut s = text.trim();
    if let Some(rest) = s.strip_prefix('-') {
        s = rest.trim_start();
    }
    loop {
        s = s.trim_start();
        if let Some(after_colon) = s.strip_prefix(':')
            && let Some(end) = after_colon.find(':')
        {
            let token = &after_colon[..end];
            if !token.is_empty()
                && token
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                s = &after_colon[end + 1..];
                continue;
            }
        }
        break;
    }
    s.trim_start_matches(|c: char| !c.is_alphanumeric() && c != '#' && c != '[' && c != '/')
        .trim()
        .to_string()
}

fn leads_with_markdown_bold_report(text: &str) -> bool {
    let mut s = text.trim();
    if let Some(rest) = s.strip_prefix('-') {
        s = rest.trim_start();
    }
    loop {
        s = s.trim_start();
        if let Some(after_colon) = s.strip_prefix(':')
            && let Some(end) = after_colon.find(':')
        {
            let token = &after_colon[..end];
            if !token.is_empty()
                && token
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                s = &after_colon[end + 1..];
                continue;
            }
        }
        break;
    }
    s.trim_start().starts_with("**")
}

fn is_single_line_artifact_noise(normalized: &str) -> bool {
    let lower = normalized.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return true;
    }
    if lower.starts_with("thread '") && lower.contains("panicked")
        || lower == "stack backtrace:"
        || lower == "backtrace:"
    {
        return true;
    }
    if lower.starts_with('[') {
        let Some(close) = lower.find(']') else {
            return false;
        };
        let tag = &lower[1..close];
        if matches!(
            tag,
            "route"
                | "preflight"
                | "session-check"
                | "queue"
                | "write"
                | "start"
                | "sync"
                | "debug"
                | "info"
                | "warn"
                | "warning"
                | "error"
                | "trace"
        ) {
            return true;
        }
    }
    false
}

/// Whether a queue prompt head is auto-drainable.
pub fn is_drainable_queue_head(text: &str) -> bool {
    is_drainable_queue_head_with_context(text, false)
}

/// True when text is a non-drainable queue-noise head.
pub fn is_noise_queue_head(text: &str, preset_supplies_directive: bool) -> bool {
    !is_drainable_queue_head_with_context(text, preset_supplies_directive)
}

pub fn is_drainable_queue_head_with_context(text: &str, preset_supplies_directive: bool) -> bool {
    if text.contains("<!-- agent:")
        || text.contains("agent:boundary")
        || leads_with_markdown_bold_report(text)
    {
        return false;
    }
    if text.contains('\n') || text.contains("```") || text.contains("~~~") {
        if !multiline_head_has_prose_lead(text) {
            return false;
        }
        // `#queuecruft`: a bulleted item that wraps onto continuation lines is
        // one head; a pasted artifact line (`[route] …`) does not become work
        // because the console paste below it was folded into the same item.
        if !text.contains("```")
            && !text.contains("~~~")
            && let Some(first) = text.lines().next()
            && is_single_line_artifact_noise(&normalize_queue_head_text(first))
        {
            return false;
        }
        return true;
    }
    let normalized = normalize_queue_head_text(text);
    if normalized.is_empty() {
        return false;
    }
    if is_single_line_artifact_noise(&normalized) {
        return false;
    }
    if extract_head_id(text).is_some() {
        return true;
    }
    if crate::queue_command::is_slash_command(&normalized) {
        return true;
    }
    if normalized.trim_end().ends_with('?') {
        return true;
    }
    if preset_supplies_directive {
        return true;
    }
    let lowered = normalized.to_ascii_lowercase();
    if lowered
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| QUEUE_DIRECTIVE_VERBS.contains(&word))
    {
        return true;
    }
    true
}

fn multiline_head_has_prose_lead(text: &str) -> bool {
    let lead = text
        .lines()
        .take_while(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("```") && !trimmed.starts_with("~~~")
        })
        .collect::<Vec<_>>()
        .join(" ");
    let normalized = normalize_queue_head_text(&lead);
    normalized
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() > 2)
        .any(|word| {
            let lowered = word.to_ascii_lowercase();
            !matches!(
                lowered.as_str(),
                "route" | "error" | "warning" | "warn" | "info" | "debug" | "trace" | "target"
            )
        })
}

/// Replace inline-code span contents with spaces so a QUOTED id or preset
/// cannot be mistaken for an active directive (`#ftquotedid`, GH #331).
///
/// An operator bug report that quotes a queue head — ``"`do [#foo]` was in the
/// queue but nothing ran"`` — is a fresh free-text prompt *about* `#foo`, not a
/// directive to run `#foo`. Scanning the raw text made such a line inherit
/// `#foo`'s backlog gating: when `#foo` was `[operator-verify]`-deferred, the
/// operator's brand-new prompt was itself classified undrainable, so the queue
/// reported `drainable_head_count: 0` while a live operator prompt sat at its
/// head and the agent answered "nothing to do".
///
/// Only balanced backtick runs are masked; an unbalanced backtick is ordinary
/// prose, so the remainder stays visible to id extraction.
pub(crate) fn mask_inline_code(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '`' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let fence = chars[i..].iter().take_while(|c| **c == '`').count();
        let mut j = i + fence;
        let mut close = None;
        while j < chars.len() {
            if chars[j] == '`' {
                let run = chars[j..].iter().take_while(|c| **c == '`').count();
                if run == fence {
                    close = Some(j);
                    break;
                }
                j += run;
                continue;
            }
            j += 1;
        }
        match close {
            Some(end) => {
                for _ in i..end + fence {
                    out.push(' ');
                }
                i = end + fence;
            }
            None => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    out
}

/// Extract the backlog id from a queue prompt like `do [#id]` or `#id`.
pub fn extract_head_id(prompt: &str) -> Option<String> {
    let scan = mask_inline_code(prompt);
    let scan = scan.as_str();
    if let Some(start) = scan.find("[#")
        && let Some(end) = scan[start + 2..].find(']')
    {
        let id = scan[start + 2..start + 2 + end].trim();
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    scan.split_whitespace()
        .find_map(|token| {
            token.strip_prefix('#').map(|rest| {
                rest.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                    .to_string()
            })
        })
        .filter(|id| !id.is_empty())
}

/// True when a queue head carries the operator's inline verdict on its own id
/// (`#opverifyanswered`).
///
/// The shape is `do [#id]: <operator text>` — the directive, then a colon, then
/// something the operator typed. Deliberately shape-based rather than
/// sentiment-based: "still broken" is as actionable as "verified. looks good",
/// and both mean a human has now looked. Agent-written heads are bare
/// `do [#id]`, so a trailing annotation is operator-authored by construction.
pub fn head_carries_operator_verdict(text: &str) -> bool {
    let scan = mask_inline_code(text);
    let rest = if let Some(start) = scan.find("[#") {
        let after = &scan[start + 2..];
        match after.find(']') {
            Some(end) => &after[end + 1..],
            None => return false,
        }
    } else {
        let Some(start) = scan.find('#') else {
            return false;
        };
        let after = &scan[start + 1..];
        let end = after
            .find(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
            .unwrap_or(after.len());
        &after[end..]
    };
    rest.trim_start()
        .strip_prefix(':')
        .is_some_and(|annotation| !annotation.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#fixlazilycodex`: a worker-claimed head is not session prompt debt;
    /// the next unclaimed head is, and a fully claimed queue has none.
    #[test]
    fn pending_head_prompt_text_excluding_claimed_skips_worker_owned_heads() {
        let content = "<!-- agent:queue -->\n- do [#a]\n- do [#b]\n<!-- /agent:queue -->\n\
                       <!-- agent:backlog -->\n- [ ] [#a] Item a.\n- [ ] [#b] Item b.\n\
                       <!-- /agent:backlog -->\n";
        let claim = |items: &[&str]| {
            crate::queue_claim::ClaimedQueueItems::from_identities(
                items
                    .iter()
                    .map(|item| crate::queue_claim::claim_identity(item)),
            )
        };
        let scope = DrainScope::InSessionLoop;
        assert_eq!(
            pending_head_prompt_text(content, scope).as_deref(),
            Some("do [#a]")
        );
        assert_eq!(
            pending_head_prompt_text_excluding_claimed(content, scope, &claim(&["do [#a]"]))
                .as_deref(),
            Some("do [#b]")
        );
        assert_eq!(
            pending_head_prompt_text_excluding_claimed(
                content,
                scope,
                &claim(&["do [#a]", "do [#b]"])
            ),
            None
        );
    }

    /// Three `agent:run` sections. The first is only an operator-gated head.
    /// Dispatching the second queue's head must not name the third as inline work.
    #[test]
    fn suppressing_dispatched_heads_stays_on_the_owner_queue() {
        let content = concat!(
            "---\nqueue: go\n---\n",
            "<!-- agent:queue id=main subagents go -->\n",
            "- 🙋 do [#blocked]:\n",
            "  - 🙋 merge the pull request [#subagent-operator-request-blocked]\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=api-prs subagents -->\n",
            "- https://example.com/pull/759\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=contracts-prs subagents -->\n",
            "- https://example.com/pull/162\n",
            "<!-- /agent:queue -->\n",
        );
        let selection = ClaimedQueueItems::none();
        let presets = frontmatter::PromptPresets::default();
        let scope = DrainScope::InSessionLoop;
        assert_eq!(
            dispatchable_head_prompt_text_for_owner(
                content, scope, &selection, &selection, &presets
            )
            .as_deref(),
            Some("https://example.com/pull/759")
        );
        let suppressed = selection.with_heads(&["https://example.com/pull/759".to_string()]);
        assert_eq!(
            dispatchable_head_prompt_text_for_owner(
                content,
                scope,
                &ClaimedQueueItems::none(),
                &suppressed,
                &presets
            ),
            None
        );
        assert_eq!(
            drainable_head_count_for_owner(
                content,
                &ClaimedQueueItems::none(),
                &suppressed,
                &presets
            ),
            0
        );
    }

    /// `#qreviewdefer`: the reported `fpe.md` shape, reduced. A `priority go`
    /// queue whose only live head names an id that is a GATED `[operator-verify]`
    /// row in `agent:review`.
    fn review_gated_queue_doc(review_row: &str) -> String {
        format!(
            concat!(
                "<!-- agent:queue preset=\"x\" priority go -->\n",
                "- ~~do [#alreadydone]~~\n",
                "- do [#fpeoptimizedprofileacceptance]\n",
                "<!-- /agent:queue -->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] [#unrelated] unrelated open work\n",
                "<!-- /agent:backlog -->\n\n",
                "<!-- agent:review -->\n",
                "{}\n",
                "<!-- /agent:review -->\n",
            ),
            review_row
        )
    }

    #[test]
    fn a_head_gated_on_a_review_row_counts_as_deferred() {
        // Before the fix this returned 0, so session-check reported
        // `no_drainable_work` (class ok, next_action no_agent_action) over a
        // queue the operator could plainly see still had an item in it.
        let doc = review_gated_queue_doc(
            "- [/] [#fpeoptimizedprofileacceptance] [operator-verify] repeat the matched inline workload",
        );
        assert_eq!(deferred_head_count(&doc), 1);
    }

    #[test]
    fn a_typed_gate_on_the_review_row_defers_the_head_too() {
        // A `strip_prefix("- [/]")` witness misses this and puts the head back
        // into the silent-stall shape.
        let doc = review_gated_queue_doc(
            "- [/release] [#fpeoptimizedprofileacceptance] [operator-verify] repeat the workload",
        );
        assert_eq!(deferred_head_count(&doc), 1);
        assert!(gated_review_ids(&doc).contains("fpeoptimizedprofileacceptance"));
    }

    #[test]
    fn an_ungated_review_row_does_not_defer_the_head() {
        // Only a GATED row is an external gate. An open row is ordinary work.
        let doc = review_gated_queue_doc("- [ ] [#fpeoptimizedprofileacceptance] no longer gated");
        assert_eq!(deferred_head_count(&doc), 0);
        assert!(gated_review_ids(&doc).is_empty());
    }

    #[test]
    fn a_struck_head_is_not_counted() {
        // `#alreadydone` is struck, and nothing else references it.
        let doc = review_gated_queue_doc("- [/] [#alreadydone] gated but struck in the queue");
        assert_eq!(deferred_head_count(&doc), 0);
    }

    /// `#opverifyanswered`: a head carrying the operator's inline verdict IS the
    /// operator action, not a wait for one, so it must not read as deferred.
    #[test]
    fn an_operator_answered_head_is_not_deferred() {
        let doc = concat!(
            "<!-- agent:queue preset=\"x\" priority go -->\n",
            "- do [#fpeoptimizedprofileacceptance]: I lift the push hold\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#unrelated] unrelated open work\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#fpeoptimizedprofileacceptance] [operator-verify] repeat the workload\n",
            "<!-- /agent:review -->\n",
        )
        .to_string();
        let answered = operator_answered_head_ids(&doc);
        assert!(
            answered.contains("fpeoptimizedprofileacceptance"),
            "fixture must actually carry an operator verdict: {answered:?}"
        );
        assert_eq!(deferred_head_count(&doc), 0);
    }

    /// The scan is scoped to `agent:review` specifically, not "any gated row".
    /// A `- [/]` item still sitting in `agent:backlog` is code-complete work
    /// awaiting review, not an external gate on the queue — counting it would
    /// report `operator_proof_required` for a head the loop or supervisor can
    /// still drain.
    #[test]
    fn a_gated_backlog_row_is_not_a_gated_review_row() {
        let doc = concat!(
            "<!-- agent:queue preset=\"x\" priority go -->\n",
            "- do [#gatedinbacklog]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [/] [#gatedinbacklog] code-complete, awaiting review\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#somethingelse] [operator-verify] unrelated gate\n",
            "<!-- /agent:review -->\n",
        );
        let ids = gated_review_ids(doc);
        assert!(
            ids.contains("somethingelse"),
            "the review row must be seen: {ids:?}"
        );
        assert!(
            !ids.contains("gatedinbacklog"),
            "a gated BACKLOG row is not an external gate: {ids:?}"
        );
        assert_eq!(
            deferred_head_count(doc),
            0,
            "the live head is gated in the backlog, not in review, so it is not operator-deferred"
        );
    }

    /// The gated-review clause must not leak into drain eligibility: widening
    /// `deferred_backlog_ids` would defer a genuinely drainable backlog item
    /// that happens to share an id with a review row.
    #[test]
    fn gated_review_ids_do_not_enter_the_backlog_deferral_set() {
        let doc = review_gated_queue_doc(
            "- [/] [#fpeoptimizedprofileacceptance] [operator-verify] repeat the workload",
        );
        assert!(
            !deferred_backlog_ids(&doc).contains("fpeoptimizedprofileacceptance"),
            "drain eligibility must stay scoped to backlog/icebox/pending"
        );
    }

    /// `#queueblockquoteintent`: a stop with heads remaining must name why.
    ///
    /// The caller used to AND the preemption into `raw_required`, so this
    /// returned `required: false, guidance: None` — indistinguishable from a
    /// genuinely drained queue. Observed with four drainable heads and BOTH
    /// `ui_outcome` and `queue_continuation_guidance` null.
    #[test]
    fn a_preempted_drain_names_its_reason_instead_of_going_silent() {
        let out = effective_continuation_output_with_preemption(true, true, false, None);
        assert!(!out.required, "a fresh exchange prompt still preempts");
        let guidance = out
            .guidance
            .expect("a suppressed drain must name its reason");
        assert!(
            guidance.contains("preempts") && guidance.contains("user_intent_prompt_changes"),
            "the reason must be actionable and point at the field that proves it: {guidance}"
        );

        // Not preempted: unchanged behaviour.
        let out = effective_continuation_output_with_preemption(true, false, false, None);
        assert!(out.required);
        assert!(out.guidance.is_some());

        // Genuinely nothing to drain stays silent — there is no reason to give.
        let out = effective_continuation_output_with_preemption(false, false, false, None);
        assert!(!out.required);
        assert!(out.guidance.is_none());

        // A start-only queue is intentionally silent to the in-session loop,
        // but it is not drained: the supervisor owns its remaining prompt.
        let supervisor_scoped = "---\nsession: sid\nqueue: start\n---\n\n\
<!-- agent:queue start -->\n\
- do [#next]\n\
<!-- /agent:queue -->\n";
        let guidance = supervisor_scoped_activation_guidance(supervisor_scoped)
            .expect("a supervisor-scoped queue with prompts must name its owner");
        assert!(guidance.contains("supervisor idle-watch"));
        assert!(guidance.contains("NOT a stalled or drained queue"));
        assert!(guidance.contains("queue: go"));

        // Recycle-yield still wins over preemption.
        let out = effective_continuation_output_with_preemption(true, true, true, None);
        assert!(!out.required);
        assert_eq!(out.guidance.as_deref(), Some(RECYCLE_YIELD_GUIDANCE));
    }

    fn doc_with_backlog(queue_prompts: &[&str], backlog_items: &[&str]) -> String {
        let queue: String = queue_prompts.iter().map(|p| format!("- {p}\n")).collect();
        let backlog: String = backlog_items.iter().map(|b| format!("{b}\n")).collect();
        format!(
            "---\nsession: sid\nagent_doc_format: template\nqueue_active: true\n---\n\n\
## Backlog\n\n<!-- agent:backlog queue=sync -->\n{backlog}<!-- /agent:backlog -->\n\n\
## Queue\n\n<!-- agent:queue auto go -->\n{queue}<!-- /agent:queue -->\n"
        )
    }

    fn doc_with_review(review_items: &[&str]) -> String {
        let review: String = review_items.iter().map(|r| format!("{r}\n")).collect();
        format!(
            "---\nsession: sid\nagent_doc_format: template\nqueue_active: true\n---\n\n\
## Review\n\n<!-- agent:review -->\n{review}<!-- /agent:review -->\n"
        )
    }

    /// `#degraded-ipc-no-stall`: the shared no-stall guidance must distinguish
    /// proven degraded editor transport from unproven IPC/direct-write fallback
    /// so neither preflight nor session-check can drift into licensing data loss.
    #[test]
    fn continuation_guidance_names_degraded_ipc_and_exhaustive_stop_list() {
        let g = CONTINUATION_NO_STALL_GUIDANCE;
        assert!(
            g.contains("CP editor delivery"),
            "must name CP delivery recovery"
        );
        assert!(
            g.contains("committed") && g.contains("session-check") && g.contains("proven"),
            "must state the successful-closeout proof"
        );
        assert!(
            g.contains("recovery=retry_without_disk_write")
                && g.contains("recovery=direct_write_fallback")
                && g.contains("not successful closeouts"),
            "must reject unproven IPC/direct-write fallback"
        );
        assert!(
            g.contains("failed closeout")
                && g.contains("unproven IPC/delivery retry condition")
                && g.contains("session-check interruption")
                && g.contains("lint-gate"),
            "must enumerate the exhaustive stop list"
        );
        assert!(
            g.contains("NOT stop reasons"),
            "must say degraded IPC / stale supervisor / accretion are NOT stop reasons"
        );
    }

    #[test]
    fn auto_clear_yield_names_an_intentional_stop_and_passes_other_outputs_through() {
        let required = effective_continuation_output(true, false, None);
        let yielded = apply_auto_clear_yield(required.clone(), true);
        assert!(!yielded.required);
        let guidance = yielded.guidance.unwrap();
        assert!(guidance.contains("END THIS TURN"), "{guidance}");
        assert!(guidance.contains("disable-auto-clear"), "{guidance}");
        assert!(guidance.contains("only compaction reduces"), "{guidance}");
        assert_eq!(apply_auto_clear_yield(required.clone(), false), required);
        let recycle = effective_continuation_output(true, true, None);
        assert_eq!(apply_auto_clear_yield(recycle.clone(), true), recycle);
    }

    #[test]
    fn continuation_guidance_explains_controller_pause_reason() {
        let g = continuation_guidance(Some("operator pause"));
        assert!(
            g.contains("queue_paused is set but is NOT a contradiction"),
            "pause-aware guidance must explain the mixed-signal shape: {g}"
        );
        assert!(
            g.contains("recorded pause reason: operator pause"),
            "pause-aware guidance must carry the controller-recorded reason: {g}"
        );
        assert!(
            g.contains(CONTINUATION_NO_STALL_GUIDANCE),
            "pause-aware guidance must preserve the normal no-stall closeout rules: {g}"
        );
    }

    #[test]
    fn effective_continuation_output_suppresses_required_for_recycle_yield() {
        let output = effective_continuation_output(true, true, Some("operator pause"));
        assert!(!output.required);
        assert_eq!(
            output.guidance.as_deref(),
            Some(RECYCLE_YIELD_GUIDANCE),
            "recycle-yield guidance should replace normal continuation guidance"
        );
    }

    #[test]
    fn effective_continuation_output_preserves_pause_guidance_when_required() {
        let output = effective_continuation_output(true, false, Some("operator pause"));
        assert!(output.required);
        let guidance = output.guidance.expect("required continuation has guidance");
        assert!(guidance.contains("recorded pause reason: operator pause"));
        assert!(guidance.contains(CONTINUATION_NO_STALL_GUIDANCE));
    }

    #[test]
    fn effective_continuation_output_has_no_guidance_when_not_required() {
        let output = effective_continuation_output(false, false, None);
        assert!(!output.required);
        assert_eq!(output.guidance, None);
    }

    fn doc_with_answered_exchange(queue_prompts: &[&str], answered: &str) -> String {
        doc_with_backlog(queue_prompts, &["- [ ] [#next] next"]).replace(
            "## Backlog",
            &format!(
                "## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: issue -- opus\n\n> **Queue prompt:** {answered}\n\nFixed and released.\n<!-- /agent:exchange -->\n\n## Backlog"
            ),
        )
    }

    /// `#syntheticstrike`: agent-doc-bugs.md 2026-09-29 14:28. The closeout
    /// committed under a synthetic cycle, so the answered free-text head was
    /// never struck and came back as the continuation head.
    #[test]
    fn required_continuation_skips_a_free_text_head_the_exchange_already_answered() {
        let answered = "Fix https://github.com/btakita/agent-doc/issues/59. release + publish";
        let content = doc_with_answered_exchange(&[answered, "do [#next]"], answered);
        let continuation = required_continuation(&content, Some(&content))
            .unwrap()
            .expect("the next real head still requires continuation");
        assert_eq!(continuation.head_prompt, "do [#next]");

        let only_answered = doc_with_answered_exchange(&[answered], answered);
        assert!(
            required_continuation(&only_answered, Some(&only_answered))
                .unwrap()
                .is_none(),
            "an answered head alone must not re-dispatch finished work"
        );
    }

    /// `#qheadresiduescope` (backlog: the continuation-head skip still matched
    /// a live free-text head against the whole exchange). The operator typed an
    /// already-answered report again after its first answer. The whole-exchange
    /// skip (no history, the behavior before the scoping) leaves it out of the
    /// continuation offer; scoped to echoes after its re-entry it is the next
    /// head. A head answered after its re-entry is still skipped.
    #[test]
    fn required_continuation_offers_a_retyped_head_answered_only_before_reentry() {
        let report = "the session list flickers when a second pane opens the same document";
        let content = doc_with_answered_exchange(&[report, "do [#next]"], report);
        let exchange = crate::queue_head_entry::exchange_component_text(&content);
        assert!(exchange.contains("Queue prompt:"), "{exchange}");

        // Before the scoping: the old echo hides the re-typed report.
        let whole = required_continuation_excluding_claimed_scoped(
            &content,
            Some(&content),
            &ClaimedQueueItems::none(),
            &whole_exchange_head_entry,
        )
        .unwrap()
        .expect("the id-backed head still requires continuation");
        assert_eq!(whole.head_prompt, "do [#next]");

        // The report re-entered the queue after that echo was written: the
        // anchor is the whole current exchange, so nothing newer answers it.
        let newer_than = |exchange: String| {
            crate::queue_head_entry::HeadEchoScope::NewerThan(
                crate::queue_head_entry::LoopPassEchoAnchor {
                    exchange,
                    kind: crate::queue_head_entry::LoopPassEchoAnchorKind::HeadEntry,
                },
            )
        };
        let reentered_after_answer = |_: &str, head: &str| {
            if head == report {
                newer_than(exchange.clone())
            } else {
                crate::queue_head_entry::HeadEchoScope::WholeExchange
            }
        };
        let scoped = required_continuation_excluding_claimed_scoped(
            &content,
            Some(&content),
            &ClaimedQueueItems::none(),
            &reentered_after_answer,
        )
        .unwrap()
        .expect("a re-typed report is drainable work");
        assert_eq!(scoped.head_prompt, report);

        // The report entered before the echo (the anchor predates the answer):
        // it is still answered residue and skipped.
        let entered_before_answer = |_: &str, _: &str| newer_than(String::new());
        let answered = required_continuation_excluding_claimed_scoped(
            &content,
            Some(&content),
            &ClaimedQueueItems::none(),
            &entered_before_answer,
        )
        .unwrap()
        .expect("the id-backed head still requires continuation");
        assert_eq!(answered.head_prompt, "do [#next]");
    }

    /// GH #327 pass 4: the continuation-head skip honours the shared echo
    /// scope, so a `loop` head the preflight residue strike keeps (no open
    /// pass, or only echoes outside the pass) is never skipped as answered.
    #[test]
    fn required_continuation_offers_a_head_no_echo_in_scope_answers() {
        let head = "upgrade the toolchain";
        let content = doc_with_answered_exchange(&[head, "do [#next]"], head);
        let exchange = crate::queue_head_entry::exchange_component_text(&content);
        for scope in [
            crate::queue_head_entry::HeadEchoScope::NoOpenPass,
            crate::queue_head_entry::HeadEchoScope::NewerThan(
                crate::queue_head_entry::LoopPassEchoAnchor {
                    exchange: exchange.clone(),
                    kind: crate::queue_head_entry::LoopPassEchoAnchorKind::PassOpened,
                },
            ),
        ] {
            let label = scope.label();
            let continuation = required_continuation_excluding_claimed_scoped(
                &content,
                Some(&content),
                &ClaimedQueueItems::none(),
                &|_: &str, _: &str| scope.clone(),
            )
            .unwrap()
            .expect("live heads require continuation");
            assert_eq!(continuation.head_prompt, head, "{label}");
        }
    }

    #[test]
    fn required_continuation_keeps_a_recurring_imperative_head_even_when_echoed() {
        let content = doc_with_answered_exchange(&["deploy to staging"], "deploy to staging");
        let continuation = required_continuation(&content, Some(&content))
            .unwrap()
            .expect("a recurring imperative stays a standing directive");
        assert_eq!(continuation.head_prompt, "deploy to staging");
    }

    #[test]
    fn required_continuation_returns_ready_auto_go_head() {
        let content = doc_with_backlog(
            &["do [#a]", "do [#b]"],
            &["- [ ] [#a] first", "- [ ] [#b] second"],
        );

        let continuation = required_continuation(&content, Some(&content))
            .unwrap()
            .expect("ready auto queue head");

        assert_eq!(continuation.head_prompt, "do [#a]");
        assert_eq!(continuation.head_id.as_deref(), Some("a"));
        assert!(continuation.reason.contains("agent:queue go"));
    }

    #[test]
    fn required_continuation_none_for_persisted_active_head_without_go() {
        let content = doc_with_backlog(&["do [#persisted]"], &["- [ ] [#persisted] first"])
            .replace("<!-- agent:queue auto go -->", "<!-- agent:queue -->");

        assert!(
            required_continuation(&content, Some(&content))
                .unwrap()
                .is_none(),
            "plain persisted-active queues are not self-driving without explicit go"
        );
    }

    #[test]
    fn required_continuation_returns_queue_go_head_without_auto() {
        let content = doc_with_backlog(&["do [#persisted]"], &["- [ ] [#persisted] first"])
            .replace("<!-- agent:queue auto go -->", "<!-- agent:queue go -->");

        let continuation = required_continuation(&content, Some(&content))
            .unwrap()
            .expect("go-mode queue head");
        assert_eq!(continuation.head_id.as_deref(), Some("persisted"));
        assert!(continuation.reason.contains("agent:queue go"));
    }

    /// `#queue-unwrapped-fenced-task`: a go-mode queue must not disappear when
    /// its sole task is direct prose surrounding a fenced quote/log.
    #[test]
    fn required_continuation_returns_unwrapped_fenced_task() {
        let task = concat!(
            "This message is from Dillon about the release gate:\n",
            "```\n",
            "Release Please must still create its PR.\n",
            "```\n\n",
            "Please describe what we should do here."
        );
        let content = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nqueue_active: true\n---\n\n\
## Queue\n\n<!-- agent:queue go -->\n{task}\n<!-- /agent:queue -->\n"
        );

        let continuation = required_continuation(&content, Some(&content))
            .unwrap()
            .expect("the recovered task is a ready continuation");
        assert_eq!(continuation.head_prompt, task);
        assert_eq!(continuation.head_id, None);
    }

    #[test]
    fn required_continuation_returns_prefixed_request_not_a_bullet_inside_its_fence() {
        let task = concat!(
            "Please fix the backend closeout failure.\n",
            "```\n",
            "- / 0 FAIL, diagnostic only\n",
            "2. #stale diagnostic only\n",
            "```"
        );
        let content = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\nqueue_active: true\n---\n\n",
                "## Queue\n\n<!-- agent:queue go -->\n",
                "- ~~do [#fixed]~~\n{}\n- \n",
                "<!-- /agent:queue -->\n"
            ),
            task
        );

        let continuation = required_continuation(&content, Some(&content))
            .unwrap()
            .expect("the prefixed fenced task is a ready continuation");
        assert_eq!(continuation.head_prompt, task);
        assert_eq!(continuation.head_id, None);
    }

    #[test]
    fn required_continuation_none_when_snapshot_head_was_modified() {
        let snapshot = doc_with_backlog(&["do [#a]"], &["- [ ] [#a] first"]);
        let current = doc_with_backlog(&["do [#b]"], &["- [ ] [#b] changed"]);

        assert!(
            required_continuation(&current, Some(&snapshot))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn required_continuation_skips_deferred_heads_and_stops_when_all_deferred() {
        let mixed = doc_with_backlog(
            &["do [#b]", "do [#c]"],
            &[
                "- [ ] [#b] [operator-verify] live drive",
                "- [ ] [#c] plain drainable",
            ],
        );
        let continuation = required_continuation(&mixed, Some(&mixed))
            .unwrap()
            .expect("drainable head remains");
        assert_eq!(continuation.head_id.as_deref(), Some("c"));

        let all_deferred = doc_with_backlog(
            &["do [#b]", "do [#d]"],
            &[
                "- [ ] [#b] [operator-verify] live drive",
                "- [ ] [#d] [operator-verify] also live",
            ],
        );
        assert!(
            required_continuation(&all_deferred, Some(&all_deferred))
                .unwrap()
                .is_none()
        );
    }

    /// `#queueawaitop`: the haiven-dev `tasks/sdk.md` backlog shapes of
    /// 2026-10-11. Operator-verify items (with or without a `[blocked: ...]`
    /// tag) and gated review rows wait on the operator; a plain open item, a
    /// `[focused-cycle]` item, and a head carrying an inline operator verdict
    /// do not.
    #[test]
    fn awaiting_operator_head_ids_names_heads_whose_agent_work_is_done() {
        let mut content = doc_with_backlog(
            &[
                "⏭\u{fe0f} do [#quirkbigint]",
                "do [#quirkinventory]",
                "do [#verdict]: verified, ship it",
                "do [#cligenport]",
                "do [#focused]",
                "do [#gate]",
            ],
            &[
                "- [ ] [#quirkbigint] [operator-verify] [blocked: #paritycorpusmain merge] PR #126 opened",
                "- [ ] [#quirkinventory] [operator-verify] haiven-sdk: COMMITTED locally, not pushed",
                "- [ ] [#verdict] [operator-verify] needs a look",
                "- [ ] 🚧 [#cligenport] port the generator",
                "- [ ] [#focused] [focused-cycle] dedicated turn",
            ],
        );
        content.push_str(
            "\n<!-- agent:review -->\n- [/] [#gate] waiting on review\n<!-- /agent:review -->\n",
        );
        let ids = awaiting_operator_head_ids(&content);
        let mut ids = ids.into_iter().collect::<Vec<_>>();
        ids.sort();
        assert_eq!(ids, vec!["gate", "quirkbigint", "quirkinventory"]);
    }

    #[test]
    fn deferred_backlog_ids_defers_only_operator_verify_for_loop() {
        let content = doc_with_backlog(
            &["do [#a]", "do [#b]", "do [#c]"],
            &[
                "- [ ] [#a] [clean-session] needs quiet",
                "- [ ] [#b] [operator-verify] live drive",
                "- [ ] [#c] plain",
            ],
        );
        let deferred = deferred_backlog_ids(&content);
        assert!(!deferred.contains("a"));
        assert!(deferred.contains("b"));
        assert!(!deferred.contains("c"));
    }

    #[test]
    fn partition_drainable_backlog_ids_skips_operator_verify_and_focused_cycle() {
        let content = doc_with_backlog(
            &[],
            &[
                "- [ ] [#clean] [clean-session] quiet",
                "- [ ] [#verify] [operator-verify] live drive",
                "- [ ] [#focused] [focused-cycle] dedicated turn",
                "- [ ] [#plain] plain drainable",
            ],
        );
        let components = element::parse(&content).unwrap();
        let contexts = collect_backlog_execution_contexts(&components, &content);
        let ids = vec![
            "clean".to_string(),
            "verify".to_string(),
            "focused".to_string(),
            "plain".to_string(),
            "missing".to_string(),
        ];

        let (drainable, skipped) = partition_drainable_backlog_ids(&ids, &contexts);

        assert_eq!(
            drainable,
            vec![
                "clean".to_string(),
                "plain".to_string(),
                "missing".to_string()
            ],
            "clean-session, plain, and context-missing ids drain in-loop"
        );
        assert_eq!(
            skipped,
            vec![
                BacklogDrainSkip {
                    id: "verify".to_string(),
                    reason: "operator_verify",
                },
                BacklogDrainSkip {
                    id: "focused".to_string(),
                    reason: "focused_cycle",
                },
            ]
        );
    }

    /// `#presetargdedup` (live, agent-doc-bugs.md 2026-09-30): `#gh-fix <url>`
    /// heads are preset invocations, not mirrors of a missing backlog item
    /// `#gh-fix`. Preflight reported `drainable_head_count: 0` and `no_changes`
    /// over three operator-queued issues.
    #[test]
    fn preset_invocation_heads_are_drainable_free_text() {
        let content = concat!(
            "---\n",
            "prompt_presets:\n",
            "  '#gh-fix': fix then close\n",
            "  '#spec': update spec\n",
            "---\n\n",
            "<!-- agent:queue preset=\"#spec\" priority go -->\n",
            "- ~~#gh-fix https://x/issues/68~~\n",
            "- #gh-fix https://x/issues/69\n",
            "- #gh-fix https://x/issues/70\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#c3d4] Other\n",
            "<!-- /agent:backlog -->\n",
        );
        assert_eq!(
            drainable_head_count(content),
            2,
            "both sibling invocations drain"
        );
        assert_eq!(
            live_drainable_continuation_head(content, DrainScope::InSessionLoop).as_deref(),
            Some("#gh-fix https://x/issues/69"),
            "the continuation head is the invocation text, never the preset name"
        );
        // A real tracked item with the same shape stays id-backed.
        assert_eq!(
            tracked_head_id("#c3d4 do it", &preset_only_identity_ids(content)).as_deref(),
            Some("c3d4")
        );
    }

    /// The preset map a `.agent-doc/project.md` declaring `'#review'` resolves
    /// to; the session document never carries it (GH #381).
    fn project_review_presets() -> frontmatter::PromptPresets {
        frontmatter::parse("---\npresets:\n  '#review': Review the change.\n---\n")
            .unwrap()
            .0
            .prompt_presets
    }

    /// GH #381: a live head that is ONLY a project-inherited preset name drains
    /// as that preset. The document has an `agent:backlog` without `#review`,
    /// and no `presets` of its own. Document-only drainability called the head
    /// a stale mirror of a missing backlog item (`drainable_head_count: 0`);
    /// the effective preset map makes it a preset invocation.
    #[test]
    fn project_inherited_preset_head_is_drainable_with_effective_presets() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- #review\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        let presets = project_review_presets();
        let none = ClaimedQueueItems::none();
        // The document-only reading is the reported failure.
        assert_eq!(drainable_head_count(content), 0);
        assert!(!preset_only_identity_ids(content).contains("review"));

        assert!(preset_only_identity_ids_with_presets(content, &presets).contains("review"));
        assert_eq!(
            drainable_head_count_excluding_claimed_with_presets(content, &none, &presets),
            1
        );
        for scope in [DrainScope::InSessionLoop, DrainScope::Supervisor] {
            assert_eq!(
                live_drainable_head_prompt_text_excluding_claimed_with_presets(
                    content, scope, &none, &presets
                )
                .as_deref(),
                Some("#review"),
                "{scope:?}"
            );
            assert_eq!(
                live_drainable_continuation_head_excluding_claimed_with_presets(
                    content, scope, &none, &presets
                )
                .as_deref(),
                Some("#review"),
                "the head routes as the invocation text, never as tracked id `review`"
            );
        }
        let continuation = required_continuation_excluding_claimed_scoped_with_presets(
            content,
            None,
            &none,
            &whole_exchange_head_entry,
            &presets,
        )
        .unwrap()
        .expect("a project-preset head requires continuation");
        assert_eq!(continuation.head_prompt, "#review");
        assert_eq!(
            dispatchable_head_prompt_text_excluding_claimed_with_presets(
                content,
                DrainScope::InSessionLoop,
                &none,
                &presets
            )
            .as_deref(),
            Some("#review")
        );
    }

    /// GH #381: the Todo-backlink exemption reads only the head's LEADING token,
    /// through markers and an optional `do`, so prose that merely mentions a
    /// preset later in the line is still audited.
    #[test]
    fn head_leads_with_preset_reads_only_the_leading_token() {
        let preset_only: HashSet<String> = ["review".to_string()].into_iter().collect();
        for head in [
            "#review",
            "#review the release notes",
            "#Review: now",
            "🚧 #review",
            "do [#review]",
            "do #review",
        ] {
            assert!(head_leads_with_preset(head, &preset_only), "{head}");
        }
        for head in [
            "Fix the #review thing",
            "#reviewer",
            "#ghost",
            "release + publish",
            "",
        ] {
            assert!(!head_leads_with_preset(head, &preset_only), "{head}");
        }
    }

    /// GH #381: a name that is BOTH a project preset and open tracked work keeps
    /// tracked-item precedence, exactly as a document-local preset does. A
    /// done item does not make the name tracked.
    #[test]
    fn project_inherited_preset_name_that_is_open_backlog_work_stays_tracked() {
        let presets = project_review_presets();
        let tracked = concat!(
            "<!-- agent:queue go -->\n",
            "- #review\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#review] Review the release notes\n",
            "<!-- /agent:backlog -->\n",
        );
        assert!(!preset_only_identity_ids_with_presets(tracked, &presets).contains("review"));
        assert_eq!(
            live_drainable_continuation_head_excluding_claimed_with_presets(
                tracked,
                DrainScope::InSessionLoop,
                &ClaimedQueueItems::none(),
                &presets
            )
            .as_deref(),
            Some("review"),
            "an id-backed head routes by its tracked id"
        );
        let done = tracked.replace("- [ ] [#review]", "- [x] [#review]");
        assert!(preset_only_identity_ids_with_presets(&done, &presets).contains("review"));
    }

    /// `#subagentintent` (live, agent-doc-bugs.md 2026-10-03): the document
    /// registered only `'#subagents'`; the operator queued `#subagent: <url>`
    /// heads. They were read as references to a missing backlog item
    /// `#subagent`, so drainability was zero and the idle supervisor never woke
    /// the session for them.
    /// `#steeringwake`: the supervisor's idle drain skips claimed heads, so a
    /// head a subagent owns is never re-dispatched into the idle parent.
    #[test]
    fn supervisor_head_skips_claimed_heads() {
        use crate::queue_claim::{ClaimedQueueItems, claim_identity};
        let content = concat!(
            "<!-- agent:queue go -->\n",
            "- #subagents: first\n",
            "- #subagents: second\n",
            "- release + publish\n",
            "<!-- /agent:queue -->\n",
        );
        let claimed = ClaimedQueueItems::from_identities([
            claim_identity("#subagents: first"),
            claim_identity("#subagents: second"),
        ]);
        assert_eq!(
            live_drainable_continuation_head_excluding_claimed(
                content,
                DrainScope::Supervisor,
                &claimed
            )
            .as_deref(),
            Some("release + publish")
        );
        let all = ClaimedQueueItems::from_identities([
            claim_identity("#subagents: first"),
            claim_identity("#subagents: second"),
            claim_identity("release + publish"),
        ]);
        assert_eq!(
            live_drainable_continuation_head_excluding_claimed(
                content,
                DrainScope::Supervisor,
                &all
            ),
            None
        );
        assert_eq!(
            live_drainable_continuation_head_excluding_claimed(
                content,
                DrainScope::Supervisor,
                &ClaimedQueueItems::none()
            ),
            live_drainable_continuation_head(content, DrainScope::Supervisor)
        );
    }

    /// GH #319: the drainable head's routing name for an id-backed head is
    /// the bare tracked id, which is NOT a claim key; the entry's prompt text
    /// is. A claim on the line matches the prompt text and never the bare id,
    /// for both the annotated `[#id]: note` and the directive `do [#id]` shape;
    /// a free-text head's two names coincide.
    #[test]
    fn drainable_head_entry_carries_the_claim_key_beside_the_bare_id() {
        for line in ["[#beyp]: approved", "do [#beyp]", "plain free text"] {
            let content = format!(
                "---\nqueue_active: true\n---\n\n<!-- agent:queue auto go -->\n- {line}\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n- [ ] [#beyp] Claimed item\n<!-- /agent:backlog -->\n"
            );
            let entry = live_drainable_continuation_head_entry(&content, DrainScope::Supervisor)
                .expect("drainable head");
            assert_eq!(entry.prompt_text, line);
            assert_eq!(
                Some(entry.head.clone()),
                live_drainable_continuation_head(&content, DrainScope::Supervisor)
            );
            let claimed = ClaimedQueueItems::none().with_heads([&line.to_string()]);
            assert!(claimed.claims(&entry.prompt_text), "{line}");
            if line.contains("[#beyp]") {
                assert_eq!(entry.head, "beyp");
                assert!(!claimed.claims(&entry.head), "bare id is not a claim key");
            } else {
                assert_eq!(entry.head, line);
            }
        }
    }

    #[test]
    fn singular_subagent_tag_heads_are_drainable_like_the_registered_plural() {
        let content = concat!(
            "---\n",
            "prompt_presets:\n",
            "  '#subagents': 'run the remaining items in subagents'\n",
            "---\n\n",
            "<!-- agent:queue priority go -->\n",
            "- #subagent: https://github.com/btakita/agent-doc/issues/116\n",
            "- #subagents: https://github.com/btakita/agent-doc/issues/117\n",
            "- #sub-agent https://github.com/btakita/agent-doc/issues/118\n",
            "- release + publish\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#c3d4] Other\n",
            "<!-- /agent:backlog -->\n",
        );
        assert_eq!(drainable_head_count(content), 4);
        for scope in [DrainScope::InSessionLoop, DrainScope::Supervisor] {
            assert_eq!(
                live_drainable_continuation_head(content, scope).as_deref(),
                Some("#subagent: https://github.com/btakita/agent-doc/issues/116"),
                "{scope:?}"
            );
        }
        // Without any registered preset the tag still carries its meaning.
        let unregistered = content.replace(
            "prompt_presets:\n  '#subagents': 'run the remaining items in subagents'\n",
            "session: sid\n",
        );
        assert_eq!(drainable_head_count(&unregistered), 4);
        // A tracked item that really is named `#subagent` stays id-backed.
        let tracked = content.replace("[#c3d4] Other", "[#subagent] real tracked item");
        assert!(!preset_only_identity_ids(&tracked).contains("subagent"));
        assert!(preset_only_identity_ids(&tracked).contains("subagents"));
    }

    #[test]
    fn collect_backlog_execution_contexts_reads_tracked_work_components() {
        let content = concat!(
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#a] [clean-session] needs a quiet session\n",
            "<!-- /agent:backlog -->\n",
            "<!-- agent:pending -->\n",
            "- [ ] [#b] [operator-verify] live drive\n",
            "<!-- /agent:pending -->\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#c] [focused-cycle] later\n",
            "<!-- /agent:icebox -->\n",
            "<!-- agent:exchange -->\n",
            "- [ ] [#d] [operator-verify] ignored\n",
            "<!-- /agent:exchange -->\n",
        );
        let components = element::parse(content).unwrap();

        let contexts = collect_backlog_execution_contexts(&components, content);

        assert!(contexts.get("a").unwrap().clean_session_required);
        assert!(contexts.get("b").unwrap().operator_verify_required);
        assert!(contexts.get("c").unwrap().focused_cycle_required);
        assert!(!contexts.contains_key("d"));
    }

    #[test]
    fn supervisor_drains_focused_cycle_but_loop_defers_it() {
        let content = doc_with_backlog(
            &["do [#f]", "do [#o]"],
            &[
                "- [ ] [#f] [focused-cycle] merge-core work",
                "- [ ] [#o] [operator-verify] live drive",
            ],
        );
        assert!(deferred_backlog_ids(&content).contains("f"));
        assert!(!supervisor_deferred_backlog_ids(&content).contains("f"));
        assert!(supervisor_deferred_backlog_ids(&content).contains("o"));
        assert_eq!(
            live_drainable_continuation_head(&content, DrainScope::Supervisor).as_deref(),
            Some("f")
        );
        assert_eq!(drainable_head_count(&content), 0);
        // `#focusedfallback`: the head the in-session loop takes over when the
        // supervisor cannot drain it.
        assert_eq!(
            supervisor_only_head_prompt_text(&content).as_deref(),
            Some("do [#f]")
        );
        // A head the in-session loop drains itself is never a fallback.
        let plain = doc_with_backlog(
            &["do [#p]", "do [#f]"],
            &["- [ ] [#p] plain", "- [ ] [#f] [focused-cycle] x"],
        );
        assert!(supervisor_only_head_prompt_text(&plain).is_none());
    }

    #[test]
    fn free_text_focused_cycle_yields_to_supervisor_and_requests_clear() {
        let head = "[focused-cycle] fix queue identity loss";
        let content = doc_with_backlog(&[head], &[]);

        assert_eq!(
            live_drainable_continuation_head(&content, DrainScope::InSessionLoop),
            None,
            "the accreted in-session loop must yield a focused free-text head"
        );
        assert_eq!(
            live_drainable_continuation_head(&content, DrainScope::Supervisor).as_deref(),
            Some(head),
            "the supervisor must pick up the same head after clearing context"
        );
        assert_eq!(drainable_head_count(&content), 0);
        assert!(head_requires_focused_cycle_in(&content, head));
        assert!(head_requires_context_reset_in(&content, head));
        assert!(!head_requires_clean_session_in(&content, head));
    }

    #[test]
    fn free_text_operator_verify_stays_deferred_from_every_agent_drain() {
        let head = "[operator-verify] verify live relay behavior";
        let content = doc_with_backlog(&[head], &[]);

        assert_eq!(
            live_drainable_continuation_head(&content, DrainScope::InSessionLoop),
            None
        );
        assert_eq!(
            live_drainable_continuation_head(&content, DrainScope::Supervisor),
            None
        );
        assert_eq!(drainable_head_count(&content), 0);
        assert!(!head_requires_context_reset_in(&content, head));
    }

    /// `#opverifyanswered`: an `[operator-verify]` head the operator answered
    /// inline is drainable — the verdict IS the human proof the tag waited for.
    /// Live repro on `tasks/agent-doc/agent-doc-bugs2.md` 2026-08-08: the
    /// operator wrote `do [#operatorverifyeyeball]: verified. looks good` and
    /// the queue still reported `drainable_head_count: 0`, so every following
    /// turn answered "nothing to do" and the verdict was never read.
    #[test]
    fn operator_answered_verify_head_becomes_drainable() {
        let head = "do [#eyeball]: verified. looks good";
        let content = doc_with_backlog(
            &[head],
            &["- [ ] [#eyeball] [operator-verify] Eyeball the live pane"],
        );

        assert!(head_carries_operator_verdict(head));
        assert_eq!(drainable_head_count(&content), 1);
        assert_eq!(
            live_drainable_continuation_head(&content, DrainScope::InSessionLoop).as_deref(),
            Some("eyeball")
        );
        assert_eq!(
            live_drainable_continuation_head(&content, DrainScope::Supervisor).as_deref(),
            Some("eyeball")
        );
    }

    #[test]
    fn operator_verdict_for_gated_review_id_is_drainable_without_open_backlog_mirror() {
        let head =
            "complete [#fpetrainpush]: I lift the push hold for the restack: Do the restack + push";
        let mut content = doc_with_review(&[
            "- [/] [#fpetrainpush] Push the restacked fpe train. Gated on: the push hold lifting.",
        ]);
        content.push_str(&format!(
            "\n## Queue\n\n<!-- agent:queue priority go -->\n- 🚧 {head}\n<!-- /agent:queue -->\n"
        ));

        assert!(head_carries_operator_verdict(head));
        assert_eq!(drainable_head_count(&content), 1);
        assert_eq!(
            live_drainable_head_prompt_text(&content, DrainScope::InSessionLoop).as_deref(),
            Some(head)
        );
    }

    /// The unanswered form must keep deferring, or the tag means nothing.
    #[test]
    fn unanswered_verify_head_stays_deferred() {
        let content = doc_with_backlog(
            &["do [#eyeball]"],
            &["- [ ] [#eyeball] [operator-verify] Eyeball the live pane"],
        );

        assert!(!head_carries_operator_verdict("do [#eyeball]"));
        assert_eq!(drainable_head_count(&content), 0);
    }

    /// A verdict does not supply a fresh cycle, so `[focused-cycle]` still
    /// yields the in-session loop and still drains under the supervisor.
    #[test]
    fn operator_verdict_does_not_satisfy_focused_cycle() {
        let content = doc_with_backlog(
            &["do [#merge]: looks fine to me"],
            &["- [ ] [#merge] [focused-cycle] merge-core rework"],
        );

        assert_eq!(drainable_head_count(&content), 0);
        assert_eq!(
            live_drainable_continuation_head(&content, DrainScope::Supervisor).as_deref(),
            Some("merge")
        );
    }

    /// `#ftquotedid`: a free-text operator prompt that QUOTES a deferred head
    /// must not inherit that head's id or its deferral. Live repro on
    /// `tasks/agent-doc/agent-doc-bugs2.md` 2026-08-08: an operator bug report
    /// containing `` `do [#operatorverifyeyeball]: ...` `` in backticks was
    /// classified as that operator-verify head and reported undrainable, so the
    /// bug report itself stalled the queue it was reporting on.
    #[test]
    fn quoted_id_in_free_text_head_is_not_the_head_id() {
        let head = "the queue stalled even though `do [#eyeball]: verified` was in it";
        let content = doc_with_backlog(
            &[head],
            &["- [ ] [#eyeball] [operator-verify] Eyeball the live pane"],
        );

        assert_eq!(extract_head_id(head), None);
        assert!(!head_carries_operator_verdict(head));
        assert_eq!(drainable_head_count(&content), 1);
        assert_eq!(
            live_drainable_continuation_head(&content, DrainScope::InSessionLoop).as_deref(),
            Some(head)
        );
    }

    /// Masking is scoped to balanced code spans: an unquoted directive id still
    /// resolves, and a stray backtick is prose, not an unterminated span.
    #[test]
    fn inline_code_masking_leaves_real_directives_intact() {
        assert_eq!(
            extract_head_id("do [#real] and mention `[#quoted]`").as_deref(),
            Some("real")
        );
        assert_eq!(
            extract_head_id("run ```[#fenced]``` then do [#real]").as_deref(),
            Some("real")
        );
        assert_eq!(
            extract_head_id("do [#real] with a stray ` tick").as_deref(),
            Some("real")
        );
        assert_eq!(extract_head_id("do [#plain]").as_deref(), Some("plain"));
        assert_eq!(extract_head_id("#bare form").as_deref(), Some("bare"));
    }

    #[test]
    fn free_text_clean_session_drains_and_carries_clear_reason() {
        let head = "[clean-session] build the release artifact";
        let content = doc_with_backlog(&[head], &[]);

        assert_eq!(
            live_drainable_continuation_head(&content, DrainScope::InSessionLoop).as_deref(),
            Some(head)
        );
        assert_eq!(drainable_head_count(&content), 1);
        assert!(head_requires_clean_session_in(&content, head));
        assert!(head_requires_context_reset_in(&content, head));
    }

    #[test]
    fn supervisor_drains_explicit_start_without_marking_continuation_required() {
        let content = concat!(
            "---\nqueue_active: true\nqueue: start\n---\n\n",
            "<!-- agent:queue -->\n",
            "/clear\n",
            "<!-- /agent:queue -->\n",
        );

        assert_eq!(live_continuation_head(content), None);
        assert_eq!(drainable_head_count(content), 0);
        assert_eq!(
            live_drainable_continuation_head(content, DrainScope::Supervisor).as_deref(),
            Some("/clear"),
            "supervisor idle drain may honor explicit one-shot start without making it a go continuation"
        );
    }

    #[test]
    fn context_reset_covers_clean_session_and_focused_cycle() {
        let content = doc_with_backlog(
            &["do [#c]", "do [#f]", "do [#o]", "do [#p]"],
            &[
                "- [ ] [#c] [clean-session] quiet",
                "- [ ] [#f] [focused-cycle] merge-core",
                "- [ ] [#o] [operator-verify] live",
                "- [ ] [#p] plain",
            ],
        );
        assert!(head_requires_context_reset_in(&content, "c"));
        assert!(head_requires_context_reset_in(&content, "do [#f]"));
        assert!(!head_requires_context_reset_in(&content, "o"));
        assert!(!head_requires_context_reset_in(&content, "p"));
        assert!(head_requires_clean_session_in(&content, "c"));
        assert!(!head_requires_clean_session_in(&content, "f"));
        assert!(!head_requires_focused_cycle_in(&content, "c"));
        assert!(head_requires_focused_cycle_in(&content, "f"));
    }

    #[test]
    fn review_phase_routed_detects_added_open_review_item() {
        let none = doc_with_review(&[]);
        let one = doc_with_review(&["- [/] [#p1] phase 1 needs live verify"]);
        let done = doc_with_review(&["- [x] [#p1] phase 1 reviewed"]);
        assert!(review_phase_routed(&none, &one));
        assert!(!review_phase_routed(&one, &one));
        assert!(!review_phase_routed(&one, &none));
        assert!(!review_phase_routed(&none, &done));
    }

    #[test]
    fn drainable_head_count_counts_only_real_drainable_heads() {
        let content = doc_with_backlog(
            &[
                "do [#b]",
                "the kanban is missing the accepted application section",
                "do [#c]",
            ],
            &[
                "- [ ] [#b] [operator-verify] live drive",
                "- [ ] [#c] plain drainable",
            ],
        );
        assert_eq!(drainable_head_count(&content), 2);
    }

    /// `#qstartinert`: an explicit `go` control must make heads drainable without
    /// a legacy `queue_active: true` flag.
    ///
    /// `control_binding` made `queue:`/the marker token canonical and stopped
    /// writing `queue_active`, so gating drainability on that flag stranded any
    /// document using the canonical control: activated, entries mirrored in, and
    /// `drainable_head_count: 0` forever. Live repro on
    /// `tasks/brookebrodack-dev.md` after convergence produced
    /// `<!-- agent:queue go -->` with no `queue_active` key.
    /// `#dagdraingate`: a head whose declared prerequisite is still OPEN must
    /// not be drainable.
    ///
    /// Order was never enforcement. Head selection takes the first *drainable*
    /// prompt, so with `#dep after=#pre` and `#pre` deferred, the iterator simply
    /// stepped over `#pre` and ran `#dep` first — the dependent before its
    /// prerequisite, regardless of queue order.
    #[test]
    fn dependent_head_is_not_drainable_while_prerequisite_is_open() {
        let content = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#dep]\n",
            "- do [#pre]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#dep] after=#pre dependent work\n",
            "- [ ] [#pre] [operator-verify] prerequisite a human must clear\n",
            "<!-- /agent:backlog -->\n",
        );

        // `#pre` is operator-verify (deferred) and `#dep` is blocked behind it,
        // so nothing is agent-drainable — previously `#dep` counted.
        assert_eq!(
            drainable_head_count(content),
            0,
            "a dependent must not become drainable by stepping over its prerequisite"
        );

        // Once the prerequisite is completed (no longer an open backlog item),
        // the dependent unblocks.
        let done = content.replace(
            "- [ ] [#pre] [operator-verify] prerequisite a human must clear\n",
            "",
        );
        assert_eq!(
            drainable_head_count(&done),
            1,
            "a met prerequisite must unblock the dependent"
        );
    }

    #[test]
    fn ordered_list_sugar_blocks_later_leaf_while_prior_leaf_is_deferred() {
        let content = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- epic\n",
            "  1. do [#pre]\n",
            "  2. do [#dep]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#pre] [operator-verify] prerequisite a human must clear\n",
            "- [ ] [#dep] dependent work\n",
            "<!-- /agent:backlog -->\n",
        );

        assert_eq!(
            after_deps_from_content(content).get("dep"),
            Some(&vec!["pre".to_string()]),
            "the runtime projection must include the compiled ordered-list edge"
        );
        assert!(
            deferred_backlog_ids(content).contains("pre"),
            "operator-verify prerequisite must remain deferred"
        );
        assert_eq!(
            drainable_head_count(content),
            0,
            "ordered-list dependency sugar must block later leaves at runtime"
        );

        let done = content.replace(
            "- [ ] [#pre] [operator-verify] prerequisite a human must clear\n",
            "",
        );
        assert_eq!(
            drainable_head_count(&done),
            1,
            "completing the preceding leaf must unblock the next ordered leaf"
        );
    }

    #[test]
    fn drainable_head_count_honors_explicit_go_without_legacy_active_flag() {
        let content = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#c]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#c] plain drainable\n",
            "<!-- /agent:backlog -->\n",
        );
        assert_eq!(
            drainable_head_count(content),
            1,
            "an explicit `go` control is the canonical activation authority"
        );
    }

    #[test]
    fn empty_scaffold_queue_is_not_a_supervisor_continuation() {
        let content = concat!(
            "---\n",
            "agent_doc_session: 7dac5d22-4f53-436a-b3ce-e0d7def9df7c\n",
            "resume:\n",
            "  claude: ec8b5a35-4854-49e4-b6bd-39c1b2908ca8\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n\n",
            "<!-- agent:boundary:519683dd -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue go -->\n",
            "- \n",
            "<!-- /agent:queue -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "## Icebox\n\n",
            "<!-- agent:icebox -->\n",
            "<!-- /agent:icebox -->\n",
        );

        assert_eq!(
            content.len(),
            536,
            "fixture must match the claimed document"
        );
        assert_eq!(
            live_drainable_continuation_head(content, DrainScope::Supervisor),
            None,
            "an empty scaffold bullet must never arm the idle watcher",
        );
        assert_eq!(drainable_head_count(content), 0);
    }

    /// `#qstartinert` guard: the explicit halts must still stop drainability.
    /// `#qbindingone` (GH #79): "explicit" means the RESOLVED control binding,
    /// so drainability agrees with preflight activation on every combination.
    #[test]
    fn drainable_head_count_respects_explicit_halts_without_legacy_flag() {
        let doc = |frontmatter: &str, marker: &str| {
            format!(
                concat!(
                    "---\nsession: sid\nagent_doc_format: template\n{}---\n\n",
                    "<!-- agent:queue{} -->\n",
                    "- do [#c]\n",
                    "<!-- /agent:queue -->\n\n",
                    "<!-- agent:backlog -->\n",
                    "- [ ] [#c] plain drainable\n",
                    "<!-- /agent:backlog -->\n",
                ),
                frontmatter, marker
            )
        };
        for (frontmatter, marker, expected, why) in [
            ("queue: pause\n", "", 0, "a bare `queue: pause` holds"),
            (
                "queue: go\n",
                " pause",
                0,
                "a marker `pause` holds the queue",
            ),
            (
                "queue_active: false\n",
                "",
                1,
                "#queuestopremove: a lone legacy `queue_active: false` is no control (go)",
            ),
            (
                "queue: stop\n",
                "",
                1,
                "#queuestopremove: a legacy `queue: stop` is no control (go)",
            ),
            (
                "queue: go\n",
                " stop",
                0,
                "#queuestopremove: a legacy `stop` marker token is the pause gesture",
            ),
            ("queue: go\n", "", 1, "`queue: go` drains"),
            (
                "queue: pause\n",
                " go",
                1,
                "GH #79: a marker `go` overrides a frontmatter hold (#qactsync)",
            ),
            (
                "queue_active: false\n",
                " go",
                1,
                "a marker `go` overrides a stale legacy halt",
            ),
        ] {
            let content = doc(frontmatter, marker);
            assert_eq!(
                drainable_head_count(&content),
                expected,
                "{why}:\n{content}"
            );
            let (fm, _) = frontmatter::parse(&content).unwrap();
            let components = element::parse(&content).unwrap();
            let queue = components.iter().find(|c| c.name == "queue").unwrap();
            assert_eq!(
                crate::control_binding::queue_control_activation(
                    &queue.attrs,
                    crate::control_binding::frontmatter_queue_control(&fm),
                ),
                expected == 1,
                "drainability must agree with preflight's activation control ({why})"
            );
        }
    }

    /// `#queuegodefaultdrain`: a queue with no control on either surface is in
    /// its default `go`, so the idle watch and the continuation detector both
    /// drain it. This is the shape every drained queue has after
    /// `#queuestopretire`, once the operator adds a new head.
    #[test]
    fn control_less_queue_is_auto_drained() {
        let content = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:queue -->\n",
            "- Remove the max character count cap\n",
            "<!-- /agent:queue -->\n",
        );
        assert_eq!(drainable_head_count(content), 1);
        assert!(
            required_continuation(content, Some(content))
                .unwrap()
                .is_some()
        );
        // An explicit hold on either surface still keeps it.
        for held in [
            content.replace("template\n---", "template\nqueue: pause\n---"),
            content.replace("<!-- agent:queue -->", "<!-- agent:queue pause -->"),
        ] {
            assert_ne!(held, content);
            assert_eq!(drainable_head_count(&held), 0, "{held}");
            assert!(
                required_continuation(&held, Some(&held)).unwrap().is_none(),
                "{held}"
            );
        }
    }

    /// GH #79 repro: a free-text head under `<!-- agent:queue priority go -->`
    /// beside a persisted `queue: stop` is drainable, so the supervisor and the
    /// in-session loop both pick it up.
    #[test]
    fn marker_go_beside_stale_queue_stop_drains_a_free_text_head() {
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\nqueue: stop\n---\n\n",
            "<!-- agent:queue priority go -->\n",
            "- What is the current status of this MR?\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(drainable_head_count(content) > 0, "{content}");
        assert!(
            required_continuation(content, Some(content))
                .unwrap()
                .is_some(),
            "the continuation detector must agree"
        );
    }

    #[test]
    fn paused_first_run_leaves_the_next_run_drainable() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:run id=first pause -->\n- held work\n<!-- /agent:run -->\n\n",
            "<!-- agent:run id=second -->\n- second work\n<!-- /agent:run -->\n\n",
            "<!-- agent:run id=third pause -->\n- third held\n<!-- /agent:run -->\n",
        );
        assert_eq!(
            live_drainable_head_prompt_text(doc, DrainScope::InSessionLoop).as_deref(),
            Some("second work")
        );
        let frontmatter = concat!(
            "---\nagent_doc_format: template\nqueue: pause\n---\n\n",
            "<!-- agent:run id=first -->\n- held work\n<!-- /agent:run -->\n\n",
            "<!-- agent:run id=second go -->\n- second work\n<!-- /agent:run -->\n\n",
            "<!-- agent:run id=third -->\n- third held\n<!-- /agent:run -->\n",
        );
        assert_eq!(
            live_drainable_head_prompt_text(frontmatter, DrainScope::InSessionLoop).as_deref(),
            Some("second work")
        );
    }

    #[test]
    fn drainable_head_count_excludes_id_head_absent_from_open_backlog() {
        // `#orphanqhead` stall-stop (sampleportal `#sy71` repro): an
        // id-backed head whose id is NOT an open backlog item is not drainable, so
        // the in-session auto-loop AND the supervisor idle-watch (which share this
        // `drainable_head_count` / `has_drainable_head` signal) STOP instead of
        // re-dispatching a dangling ref forever. Halt-safety is preserved — the
        // head stays queued for the operator to resolve; it is simply not
        // auto-re-dispatched, and it is never auto-marked-done.
        let mixed = doc_with_backlog(
            &[":round_pushpin: [#sy71]", "do [#hmw9]"],
            &["- [ ] [#hmw9] a real open task"],
        );
        // Only the tracked #hmw9 head is drainable; the dangling #sy71 is excluded.
        assert_eq!(drainable_head_count(&mixed), 1);

        // When the dangling head is the sole live prompt, the queue is fully
        // undrainable, so `queue_continuation_required` (active && count > 0) is
        // false and the loop stops.
        let only_dangling = doc_with_backlog(
            &[":round_pushpin: [#sy71]"],
            &["- [ ] [#hmw9] a real open task"],
        );
        assert_eq!(drainable_head_count(&only_dangling), 0);
    }

    #[test]
    fn drainability_classifies_directive_vs_noise() {
        assert!(is_drainable_queue_head(":round_pushpin: do [#fcc0]"));
        assert!(is_drainable_queue_head("- :pushpin: Fix the submit bug"));
        assert!(is_drainable_queue_head("- /model sonnet"));
        assert!(is_drainable_queue_head("deploy"));
        assert!(!is_drainable_queue_head("- "));
        assert!(!is_drainable_queue_head(":pushpin:"));
        assert!(!is_drainable_queue_head("[route] target tmux session: 0"));
        assert!(!is_drainable_queue_head("```\n[route] target\n```"));
        assert!(is_drainable_queue_head(
            "JB Run Agent Doc should self-heal.\n```\n[route] target\n```"
        ));
    }

    #[test]
    fn recurring_imperative_head_is_narrow() {
        for head in ["deploy", "commit + push", "#commit-push"] {
            assert!(is_recurring_imperative_head(head));
            assert!(is_drainable_queue_head(head));
        }
        for head in ["fix the deploy script", "release notes should render"] {
            assert!(!is_recurring_imperative_head(head));
            assert!(is_drainable_queue_head(head));
        }
    }

    #[test]
    fn queue_stale_noise_counts_prompt_and_freeform_noise() {
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "## Queue\n\n",
            "<!-- agent:queue priority go -->\n",
            "- [route] target tmux session: 0\n",
            "console paste\n",
            "- do [#real]\n",
            "<!-- /agent:queue -->\n"
        );
        assert_eq!(queue_stale_noise_lines(content), 1);
    }

    #[test]
    fn extract_head_id_handles_bracket_and_bare() {
        assert_eq!(extract_head_id("do [#abc] thing").as_deref(), Some("abc"));
        assert_eq!(
            extract_head_id("#bare-id do it").as_deref(),
            Some("bare-id")
        );
        assert_eq!(extract_head_id("no id here"), None);
    }

    /// `#queueclaim`: a head dispatched to a subagent is in flight elsewhere,
    /// so the in-session drainability filter skips it and the continuation
    /// lands on the next unclaimed head.
    #[test]
    fn drainability_skips_a_claimed_head() {
        use crate::queue_claim::{ClaimedQueueItems, claim_identity};
        let content = doc_with_backlog(
            &["do [#a]", "do [#b]"],
            &["- [ ] [#a] first", "- [ ] [#b] second"],
        );
        let claimed = ClaimedQueueItems::from_identities([claim_identity("🚧 do [#a]")]);
        let continuation =
            required_continuation_excluding_claimed(&content, Some(&content), &claimed)
                .unwrap()
                .expect("the unclaimed head still drains");
        assert_eq!(continuation.head_prompt, "do [#b]");
        assert_eq!(drainable_head_count(&content), 2);
        assert_eq!(
            drainable_head_count_excluding_claimed(&content, &claimed),
            1
        );
        assert_eq!(claimed_head_count(&content, &claimed), 1);
    }

    #[test]
    fn required_continuation_strips_transient_in_progress_marker() {
        let content = doc_with_backlog(&["🚧 do [#a]"], &["- [ ] [#a] first"]);

        let continuation = required_continuation(&content, Some(&content))
            .unwrap()
            .expect("marked head remains drainable");

        assert_eq!(continuation.head_prompt, "do [#a]");
        assert_eq!(continuation.head_id.as_deref(), Some("a"));
    }

    #[test]
    fn live_queue_head_texts_strip_lifecycle_projection() {
        let content = doc_with_backlog(
            &["⚠️ do [#a]", "🚧 finish the free-text task"],
            &["- [ ] [#a] first"],
        );

        assert_eq!(
            live_queue_head_texts(&content).unwrap(),
            vec!["do [#a]", "finish the free-text task"]
        );
    }

    #[test]
    fn required_continuation_preserves_unmarked_head_whitespace() {
        let content = doc_with_backlog(&["  /clear  "], &[]);

        let continuation = required_continuation(&content, Some(&content))
            .unwrap()
            .expect("unmarked head remains drainable");

        assert_eq!(continuation.head_prompt, "  /clear  ");
    }

    /// `#queueclaim`: when every remaining head is claimed, no continuation is
    /// required — the loop waits on its workers instead of re-entering.
    #[test]
    fn all_claimed_heads_require_no_continuation() {
        use crate::queue_claim::{ClaimedQueueItems, claim_identity};
        let content = "---\nsession: sid\nagent_doc_format: template\n---\n\n\
## Queue\n\n<!-- agent:queue go -->\n\
- #gh-fix https://github.com/o/r/issues/109\n\
- #gh-fix https://github.com/o/r/issues/110\n\
<!-- /agent:queue -->\n"
            .to_string();
        assert!(
            required_continuation(&content, Some(&content))
                .unwrap()
                .is_some()
        );
        let claimed = ClaimedQueueItems::from_identities([
            claim_identity("#gh-fix https://github.com/o/r/issues/109"),
            claim_identity("📌 #gh-fix https://github.com/o/r/issues/110"),
        ]);
        assert!(
            required_continuation_excluding_claimed(&content, Some(&content), &claimed)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            drainable_head_count_excluding_claimed(&content, &claimed),
            0
        );
        assert_eq!(claimed_head_count(&content, &claimed), 2);
        let live = live_queue_head_identities(&content).unwrap();
        assert!(live.contains(&claim_identity("#gh-fix https://github.com/o/r/issues/110")));
    }
}

/// `#fr79` — every active tracked-work id the document still knows about, across
/// `backlog`, `icebox` and `pending`, lowercased.
///
/// Used to reconcile the queue in the direction the backlog→queue mirror cannot:
/// finding `do [#id]` heads whose id no longer exists anywhere in the document.
pub fn active_tracked_ids(content: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    let Ok(components) = element::parse(content) else {
        return ids;
    };
    for comp in &components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        for (id, _ctx) in backlog::active_item_execution_contexts(body) {
            ids.insert(id.to_ascii_lowercase());
        }
    }
    ids
}

/// `#fr79` — decide whether an id-backed queue head is *orphaned*: its id is not
/// an active tracked item, not reaped into `agent:done`, and not a gated review
/// item.
///
/// The backlog→queue mirror is self-healing in one direction (an open
/// queue-attr backlog id always regains a head) and the auto-strike covers ids
/// resolved into `done`/review. Neither covers a head whose id has simply
/// ceased to exist — an operator deletion, a renamed id, or a lost write. Such a
/// head is undrainable forever: nothing can resolve it, and it occupies the
/// drain position on every cycle.
///
/// Deliberately conservative. It fires only when the id is absent from ALL
/// three sets, so an id that is merely deferred, gated, iceboxed or pending is
/// never treated as orphaned; when in doubt the head is kept.
pub fn queue_head_id_is_orphaned(
    id: &str,
    active_tracked: &HashSet<String>,
    done_ids: &HashSet<String>,
    gated_ids: &HashSet<String>,
) -> bool {
    let id = id.trim().to_ascii_lowercase();
    if id.is_empty() {
        return false;
    }
    !active_tracked.contains(&id) && !done_ids.contains(&id) && !gated_ids.contains(&id)
}

/// Whether a dangling queue head may be auto-struck (`#fr79`).
///
/// This is the guard the earlier attempt was missing. Wiring
/// [`queue_head_id_is_orphaned`] straight into the preflight strike pass struck
/// REAL heads and failed 12 preflight tests (for example
/// `preflight_rebases_active_queue_head_change_without_mid_edit_evidence` lost
/// `do [#newhead]` to `source=orphaned_no_tracked_item`). The reason is
/// structural, not a tuning problem: **a queue head is not required to have a
/// backlog item.** Operators author `do [#id]` heads directly, and a document
/// need not carry a backlog component at all, so "no tracked item" cannot mean
/// "orphaned" without deleting legitimate queued work.
///
/// Provenance is what separates the two cases:
///
/// - **Mirror-created head whose backlog id vanished** — real drift. The mirror
///   put it there *because* an item existed; the item is gone, so the head can
///   never be resolved. Safe to strike.
/// - **Operator-authored head** — authoritative on its own (`#qauthorder`).
///   Never strike, no matter what the backlog says.
/// - **Unknown provenance** — treated as operator-authored. This is what makes
///   the rollout safe by construction: documents predating the provenance table
///   have no rows, so nothing becomes strikable until the mirror records it.
///
/// Both conditions must hold, so the conservative default survives: an id that
/// is merely deferred, gated, iceboxed or pending is never orphaned, and a head
/// the mirror never created is never struck.
pub fn queue_head_is_strikable_drift(
    id: &str,
    active_tracked: &HashSet<String>,
    done_ids: &HashSet<String>,
    gated_ids: &HashSet<String>,
    mirror_created_identities: &HashSet<String>,
) -> bool {
    if !queue_head_id_is_orphaned(id, active_tracked, done_ids, gated_ids) {
        return false;
    }
    let normalized = id.trim().to_ascii_lowercase();
    mirror_created_identities.contains(&normalized)
}

#[cfg(test)]
mod fr79_orphan_reconcile_tests {
    use super::*;

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// The core `#fr79` distinction: same dangling id, opposite outcome
    /// depending on who created the head.
    #[test]
    fn only_mirror_created_dangling_heads_are_strikable() {
        let none = set(&[]);

        assert!(
            queue_head_is_strikable_drift("sy71", &none, &none, &none, &set(&["sy71"])),
            "a mirror-created head whose backlog id vanished is real drift"
        );
        assert!(
            !queue_head_is_strikable_drift("newhead", &none, &none, &none, &set(&["sy71"])),
            "an operator-authored head is authoritative on its own (#qauthorder)"
        );
    }

    /// Unknown provenance must behave exactly like operator-authored — this is
    /// the property that makes existing documents safe on upgrade.
    #[test]
    fn unknown_provenance_is_never_struck() {
        let none = set(&[]);
        assert!(
            !queue_head_is_strikable_drift("anything", &none, &none, &none, &none),
            "with no provenance recorded, nothing may be struck"
        );
    }

    /// Provenance never overrides the conservative orphan test — a head whose id
    /// still exists anywhere is kept even if the mirror created it.
    #[test]
    fn mirror_provenance_does_not_strike_a_live_id() {
        let mirrored = set(&["alpha"]);
        let none = set(&[]);

        assert!(
            !queue_head_is_strikable_drift("alpha", &set(&["alpha"]), &none, &none, &mirrored),
            "an id still tracked is not drift"
        );
        assert!(
            !queue_head_is_strikable_drift("alpha", &none, &set(&["alpha"]), &none, &mirrored),
            "an id resolved into done is handled by the existing auto-strike, not this path"
        );
        assert!(
            !queue_head_is_strikable_drift("alpha", &none, &none, &set(&["alpha"]), &mirrored),
            "a gated id is still live work"
        );
    }

    /// The regression the earlier attempt hit, pinned directly.
    #[test]
    fn operator_authored_new_head_survives_an_empty_backlog() {
        let none = set(&[]);
        assert!(
            !queue_head_is_strikable_drift("newhead", &none, &none, &none, &none),
            "preflight_rebases_active_queue_head_change_without_mid_edit_evidence must keep \
             `do [#newhead]` — a document need not carry a backlog component at all"
        );
    }

    #[test]
    fn a_head_whose_id_vanished_is_orphaned() {
        assert!(queue_head_id_is_orphaned(
            "sy71",
            &set(&["other"]),
            &set(&[]),
            &set(&[])
        ));
    }

    #[test]
    fn open_done_and_gated_ids_are_never_orphaned() {
        let active = set(&["open1"]);
        let done = set(&["done1"]);
        let gated = set(&["gated1"]);
        for id in ["open1", "done1", "gated1"] {
            assert!(
                !queue_head_id_is_orphaned(id, &active, &done, &gated),
                "{id} is still known to the document and must keep its head"
            );
        }
    }

    #[test]
    fn matching_is_case_insensitive_and_ignores_empty_ids() {
        assert!(!queue_head_id_is_orphaned(
            "OPEN1",
            &set(&["open1"]),
            &set(&[]),
            &set(&[])
        ));
        assert!(!queue_head_id_is_orphaned(
            "  ",
            &set(&[]),
            &set(&[]),
            &set(&[])
        ));
    }

    /// `active_tracked_ids` must span every component that can hold a live item,
    /// or reconciliation would strike heads for iceboxed/pending work.
    #[test]
    fn active_tracked_ids_span_backlog_icebox_and_pending() {
        let doc = concat!(
            "<!-- agent:backlog -->\n- [ ] [#inbacklog] work\n<!-- /agent:backlog -->\n",
            "<!-- agent:icebox -->\n- [ ] [#inicebox] later\n<!-- /agent:icebox -->\n",
            "<!-- agent:pending -->\n- [ ] [#inpending] soon\n<!-- /agent:pending -->\n",
        );
        let ids = active_tracked_ids(doc);
        for id in ["inbacklog", "inicebox", "inpending"] {
            assert!(ids.contains(id), "{id} must count as tracked; got {ids:?}");
        }
    }
}
