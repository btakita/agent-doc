//! Pure queue-continuation drainability policy.
//!
//! This module owns content-only decisions for active queue continuation,
//! drainable heads, deferred backlog ids, recurring imperative heads, and queue
//! noise classification. Callers own file IO, controller state, and marker persistence.

use std::collections::{HashMap, HashSet};

use agent_doc_document::queue_projection::strip_in_progress_marker;
use agent_doc_element::element;
use agent_doc_element_backlog::backlog;
use agent_doc_frontmatter::frontmatter;
use anyhow::{Context, Result};

use crate::document_queue::{self, QueueEntry, QueuePrompt};

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
                "queue_paused is set but is NOT a contradiction with queue_continuation_required — an accepted `admin queue pause` suppresses ONLY the unattended supervisor idle-watch auto-injection (the flood guard); the attended in-session loop remains the legitimate single-owner drain and must keep draining proven-closeout work. Do not stop the loop on the pause; to actually stop the in-session loop use `queue: stop` frontmatter or a `--- stop` fence, not pause ({reason_clause}). {CONTINUATION_NO_STALL_GUIDANCE}"
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
    if recycle_yield_pending {
        return EffectiveContinuationOutput {
            required: false,
            guidance: Some(RECYCLE_YIELD_GUIDANCE.to_string()),
        };
    }
    if raw_required {
        return EffectiveContinuationOutput {
            required: true,
            guidance: Some(continuation_guidance(pause_reason)),
        };
    }
    EffectiveContinuationOutput {
        required: false,
        guidance: None,
    }
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
    let (fm, _) = frontmatter::parse(content)?;
    if fm.queue_active != Some(true) {
        return Ok(None);
    }
    let components = element::parse(content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(None);
    };
    if !explicit_go_mode(&fm, &queue_component.attrs) {
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
        && let Some(snapshot_queue) = snapshot_components
            .iter()
            .find(|component| component.name == "queue")
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

    let Some(head) = drainable_head_prompt_for_scope(content, DrainScope::InSessionLoop) else {
        return Ok(None);
    };
    let head_prompt = head.text;
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
    deferred_backlog_ids_scoped(content, DrainScope::InSessionLoop)
}

/// Active backlog ids, lowercased, that are not drainable by the supervisor.
pub fn supervisor_deferred_backlog_ids(content: &str) -> HashSet<String> {
    deferred_backlog_ids_scoped(content, DrainScope::Supervisor)
}

fn deferred_backlog_ids_scoped(content: &str, scope: DrainScope) -> HashSet<String> {
    let mut deferred = HashSet::new();
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
            if undrainable {
                deferred.insert(id.to_ascii_lowercase());
            }
        }
    }
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
    head_id_in_set(head, &clean_session_backlog_ids(content))
}

/// Whether queue `head` maps to a `[focused-cycle]` backlog item.
pub fn head_requires_focused_cycle_in(content: &str, head: &str) -> bool {
    head_id_in_set(head, &focused_cycle_backlog_ids(content))
}

/// Whether queue `head` maps to a backlog item requiring supervisor context reset.
pub fn head_requires_context_reset_in(content: &str, head: &str) -> bool {
    head_id_in_set(head, &context_reset_backlog_ids(content))
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

/// Count active queue prompt heads whose backlog id is deferred in-session.
pub fn deferred_head_count(content: &str) -> usize {
    let Some((_, entries)) = queue_component_entries(content) else {
        return 0;
    };
    let deferred_ids = deferred_backlog_ids(content);
    entries
        .iter()
        .filter_map(|entry| match entry {
            QueueEntry::Prompt(prompt) => extract_head_id(&prompt.text),
            _ => None,
        })
        .filter(|id| deferred_ids.contains(&id.to_ascii_lowercase()))
        .count()
}

/// Active queue continuation head, without drainability filtering.
pub fn live_continuation_head(content: &str) -> Option<String> {
    let (_, activation) = active_queue(content)?;
    let head = document_queue::first_prompt(&activation.entries_after)?;
    Some(extract_head_id(&head.text).unwrap_or_else(|| head.text.trim().to_string()))
}

/// First drainable active queue prompt for `scope`.
pub fn drainable_head_prompt_for_scope(content: &str, scope: DrainScope) -> Option<QueuePrompt> {
    let (queue_facts, activation) =
        active_queue_for_supervisor_start(content, matches!(scope, DrainScope::Supervisor))?;
    let open_backlog = open_backlog_ids_from_content(content);
    let deferred_ids = match scope {
        DrainScope::InSessionLoop => deferred_backlog_ids(content),
        DrainScope::Supervisor => supervisor_deferred_backlog_ids(content),
    };
    first_drainable_head(
        &activation.entries_after,
        open_backlog.as_ref(),
        &deferred_ids,
        &after_deps_from_content(content),
        queue_facts.preset_supplies_directive,
    )
    .cloned()
}

/// Live drainable active queue head for `scope`.
pub fn live_drainable_continuation_head(content: &str, scope: DrainScope) -> Option<String> {
    let head = drainable_head_prompt_for_scope(content, scope)?;
    let stripped = strip_in_progress_marker(&head.text);
    Some(extract_head_id(&stripped).unwrap_or(stripped))
}

/// Count agent-drainable heads in the active queue for the in-session loop.
pub fn drainable_head_count(content: &str) -> usize {
    let Some((queue_facts, activation)) = active_queue(content) else {
        return 0;
    };
    let open_backlog = open_backlog_ids_from_content(content);
    let deferred_ids = deferred_backlog_ids(content);
    let after_deps = after_deps_from_content(content);
    activation
        .entries_after
        .iter()
        .filter(|entry| match entry {
            QueueEntry::Prompt(prompt) => head_is_drainable(
                &prompt.text,
                open_backlog.as_ref(),
                &deferred_ids,
                &after_deps,
                queue_facts.preset_supplies_directive,
            ),
            _ => false,
        })
        .count()
}

/// Count active queue entries that are predicate-proven non-drainable noise.
pub fn queue_stale_noise_lines(content: &str) -> usize {
    let Some((queue_facts, entries)) = queue_component_entries(content) else {
        return 0;
    };
    entries
        .iter()
        .filter(|entry| match entry {
            QueueEntry::Prompt(prompt) => {
                is_noise_queue_head(&prompt.text, queue_facts.preset_supplies_directive)
            }
            QueueEntry::Freeform(line) => document_queue::is_noise_freeform_line(line),
            _ => false,
        })
        .count()
}

#[derive(Debug, Clone, Copy)]
struct QueueFacts {
    has_auto: bool,
    marker_go: bool,
    marker_start: bool,
    preset_supplies_directive: bool,
}

fn queue_component_entries(content: &str) -> Option<(QueueFacts, Vec<QueueEntry>)> {
    let components = element::parse(content).ok()?;
    let queue_component = components.iter().find(|c| c.name == "queue")?;
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = document_queue::parse(body).ok()?;
    Some((
        QueueFacts {
            has_auto: document_queue::has_auto_attr(&queue_component.attrs),
            marker_go: queue_component.attrs.contains_key("go"),
            marker_start: queue_component.attrs.contains_key("start"),
            preset_supplies_directive: queue_component.attrs.contains_key("preset"),
        },
        entries,
    ))
}

fn active_queue(content: &str) -> Option<(QueueFacts, document_queue::QueueActivation)> {
    active_queue_for_supervisor_start(content, false)
}

fn active_queue_for_supervisor_start(
    content: &str,
    allow_supervisor_start: bool,
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
    // queue finishes — so it still stops drainability, as does `queue: stop`.
    if fm.queue_active == Some(false) {
        return None;
    }
    if fm
        .queue
        .as_deref()
        .is_some_and(|raw| raw.trim().eq_ignore_ascii_case("stop"))
    {
        return None;
    }
    let (queue_facts, entries) = queue_component_entries(content)?;
    let explicit_go = queue_facts.marker_go
        || fm
            .queue
            .as_deref()
            .is_some_and(|raw| raw.trim().eq_ignore_ascii_case("go"));
    let explicit_start = allow_supervisor_start
        && (queue_facts.marker_start
            || fm
                .queue
                .as_deref()
                .is_some_and(|raw| raw.trim().eq_ignore_ascii_case("start")));
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

fn explicit_go_mode(
    fm: &frontmatter::Frontmatter,
    attrs: &std::collections::HashMap<String, String>,
) -> bool {
    attrs.contains_key("go")
        || fm
            .queue
            .as_deref()
            .is_some_and(|raw| raw.trim().eq_ignore_ascii_case("go"))
}

fn first_drainable_head<'a>(
    entries_after: &'a [QueueEntry],
    open_backlog_ids: Option<&HashSet<String>>,
    deferred_ids: &HashSet<String>,
    after_deps: &HashMap<String, Vec<String>>,
    preset_supplies_directive: bool,
) -> Option<&'a QueuePrompt> {
    entries_after.iter().find_map(|entry| match entry {
        QueueEntry::Prompt(prompt) => {
            if head_is_drainable(
                &prompt.text,
                open_backlog_ids,
                deferred_ids,
                after_deps,
                preset_supplies_directive,
            ) {
                Some(prompt)
            } else {
                None
            }
        }
        _ => None,
    })
}

fn head_is_drainable(
    text: &str,
    open_backlog_ids: Option<&HashSet<String>>,
    deferred_ids: &HashSet<String>,
    after_deps: &HashMap<String, Vec<String>>,
    preset_supplies_directive: bool,
) -> bool {
    let drainable = if preset_supplies_directive {
        is_drainable_queue_head_with_context(text, true)
    } else {
        is_drainable_queue_head(text)
    };
    if !drainable {
        return false;
    }
    match extract_head_id(text) {
        Some(id) => {
            let norm = id.to_ascii_lowercase();
            if deferred_ids.contains(&norm) {
                return false;
            }
            match open_backlog_ids {
                Some(open) => {
                    if !open.contains(&norm) {
                        return false;
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

/// Extract the backlog id from a queue prompt like `do [#id]` or `#id`.
pub fn extract_head_id(prompt: &str) -> Option<String> {
    if let Some(start) = prompt.find("[#")
        && let Some(end) = prompt[start + 2..].find(']')
    {
        let id = prompt[start + 2..start + 2 + end].trim();
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    prompt
        .split_whitespace()
        .find_map(|token| {
            token.strip_prefix('#').map(|rest| {
                rest.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                    .to_string()
            })
        })
        .filter(|id| !id.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// `#qstartinert` guard: the explicit halts must still stop drainability.
    #[test]
    fn drainable_head_count_respects_explicit_halts_without_legacy_flag() {
        let stopped = concat!(
            "---\nsession: sid\nagent_doc_format: template\nqueue: stop\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#c]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#c] plain drainable\n",
            "<!-- /agent:backlog -->\n",
        );
        assert_eq!(
            drainable_head_count(stopped),
            0,
            "`queue: stop` must dominate a stale marker `go`"
        );

        // A drained queue writes `queue_active: false`; that remains an explicit halt.
        let cleared = stopped.replace("queue: stop", "queue_active: false");
        assert_eq!(
            drainable_head_count(&cleared),
            0,
            "a persisted `queue_active: false` must still halt the drain"
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
            "<!-- agent:queue auto -->\n",
            "- [route] target tmux session: 0\n",
            "console paste\n",
            "- do [#real]\n",
            "<!-- /agent:queue -->\n"
        );
        assert_eq!(queue_stale_noise_lines(content), 2);
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
        assert!(!queue_head_id_is_orphaned("OPEN1", &set(&["open1"]), &set(&[]), &set(&[])));
        assert!(!queue_head_id_is_orphaned("  ", &set(&[]), &set(&[]), &set(&[])));
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
