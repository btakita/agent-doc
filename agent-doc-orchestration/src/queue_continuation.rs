//! # Module: queue_continuation
//!
//! Binary-owned "queue continuation required" final gate
//! (`#codex-auto-queue-stalled-final-gate`).
//!
//! Codex auto-queue continuation historically depended on the `codex-stop` hook
//! finding tracked in-memory session state and then calling
//! `active_auto_queue_prompt`. That is too fragile for the live failure mode:
//! the Stop hook can miss the document when `UserPromptSubmit` did not persist
//! state for the exact API/session/root shape, or when the turn closed through a
//! manual / recovery path after a recursive direct-invocation rejection. A clean
//! `session-check` is not enough either — a committed document can still owe an
//! auto-queue continuation.
//!
//! The only durable proof after closeout is the document itself
//! (`queue_active: true` and a ready head) plus the durable marker this module
//! persists at successful closeout. `auto` is only a *start* trigger; once a
//! queue is active, continuation is driven by `queue_active: true`, so a
//! persisted-active `agent:queue` (no `auto` attribute) is equally eligible
//! (`#active-queue-persisted-no-continue`). The detector here is the single
//! shared source of truth; `session-check`, the `codex-stop` hook, and the
//! closeout paths all consult it instead of duplicating the activation
//! reasoning.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A required auto-queue continuation: the document closed cleanly but a ready
/// `agent:queue auto` head remains, so a Codex-managed turn must continue with
/// `agent-doc <FILE>` instead of sending a final answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueContinuation {
    pub head_prompt: String,
    pub head_id: Option<String>,
    pub reason: String,
}

/// Shared non-stall guidance surfaced wherever `queue_continuation_required ==
/// true`. Centralizing the wording keeps preflight JSON
/// (`queue_continuation_guidance`) and `session-check` stdout in agreement
/// (`#degraded-ipc-no-stall`).
///
/// The failure this guards against: a `finalize` that reached `committed` +
/// `session-check ok` through the **file-IPC fallback** (socket ack timeouts /
/// a stale or wedged route-owned supervisor) is a *successful* closeout — the
/// in-session loop does not depend on the socket, because the file-IPC patch
/// queue + CRDT merge already carried the commit. The agent must not invent a
/// stop reason from the degraded transport. The ONLY closeout states that stop
/// the loop are a FAILED closeout, a `session-check` interruption, or a
/// `lint-gate` block. Degraded / file-IPC-fallback IPC, a stale or wedged
/// supervisor, high session-accretion, a `semantic_completion_match` warning,
/// and a `[clean-session]` head wanting "fresh context" are NOT stop reasons.
pub const CONTINUATION_NO_STALL_GUIDANCE: &str = "queue continuation required — keep draining. A closeout that reached committed + session-check ok is successful EVEN via the file-IPC fallback (socket degraded / ack timeouts / stale supervisor): the in-session loop does not depend on the socket, the file-IPC patch queue + CRDT merge already carried the commit. Do NOT invent a stop reason from degraded transport. Only a failed closeout, a session-check interruption, or a lint-gate block stops the loop. Degraded IPC, a stale/wedged supervisor, high session-accretion, and semantic_completion_match warnings are NOT stop reasons.";

/// Detect whether `file` currently requires queue continuation.
///
/// True only when: frontmatter `queue_active: true`,
/// [`crate::queue::resolve_activation`] is active, the head is a real prompt
/// (not a stop fence or future time gate), and the head was not edited between
/// the committed snapshot and the file.
///
/// `auto` is a *start* trigger only; once a queue is active (`queue_active:
/// true`) continuation is driven by the active state, not the opening-tag
/// attribute, so a persisted-active `agent:queue` (no `auto`) is equally
/// eligible (`#active-queue-persisted-no-continue`). An inactive plain queue
/// never reaches here because the `queue_active` guard above fails first. This
/// mirrors the codex-hook `active_auto_queue_prompt` logic in one shared,
/// testable place.
pub fn detect(file: &Path) -> Result<Option<QueueContinuation>> {
    let content = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    detect_in_content(file, &content)
}

fn detect_in_content(file: &Path, content: &str) -> Result<Option<QueueContinuation>> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let (fm, _) = crate::frontmatter::parse_for_file_with_context(content, file, &rc)?;
    if fm.queue_active != Some(true) {
        return Ok(None);
    }
    let components = crate::component::parse(content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(None);
    };
    // `auto` is a start trigger only — continuation is gated on `queue_active:
    // true` (checked above), so a persisted-active queue without `auto` still
    // continues (`#active-queue-persisted-no-continue`).
    let has_auto = crate::queue::has_auto_attr(&queue_component.attrs);

    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = crate::queue::parse(body).context("queue continuation: failed to parse queue")?;
    let activation = crate::queue::resolve_activation(&entries, has_auto, false, true);
    if !activation.active
        || crate::queue::has_stop_fence_at_head(&activation.entries_after)
        || crate::queue::time_gate_at_head(&activation.entries_after).is_some()
    {
        return Ok(None);
    }

    // A head edited between the committed snapshot and the file is not a clean
    // continuation — the operator changed the next prompt, so defer to the
    // normal preflight/halt path rather than forcing continuation.
    if let Some(snapshot_content) = crate::snapshot::load(file)?
        && let Ok(snapshot_components) = crate::component::parse(&snapshot_content)
        && let Some(snapshot_queue) = snapshot_components
            .iter()
            .find(|component| component.name == "queue")
    {
        let snapshot_body = &snapshot_content[snapshot_queue.open_end..snapshot_queue.close_start];
        if let Ok(snapshot_entries) = crate::queue::parse(snapshot_body) {
            let snapshot_has_auto = crate::queue::has_auto_attr(&snapshot_queue.attrs);
            let snapshot_activation =
                crate::queue::resolve_activation(&snapshot_entries, snapshot_has_auto, false, true);
            if crate::queue::detect_head_prompt_modified(
                &snapshot_activation.entries_after,
                &activation.entries_after,
            ) {
                return Ok(None);
            }
        }
    }

    // #goqueuestall: continuation is computed over the DRAINABLE head set only.
    // A head whose backlog id carries `[operator-verify]` (never agent-drainable)
    // is deferred, not a stall (`#qcontdrain`: `[clean-session]` drains in place).
    // Walk past deferred heads; if every remaining head is deferred, continuation
    // is NOT required so the session does not perpetually re-converge an
    // undrainable queue.
    let open_backlog = open_backlog_ids_from_content(content);
    let deferred_ids = deferred_backlog_ids(content);
    let head = first_drainable_head(&activation.entries_after, open_backlog.as_ref(), &deferred_ids);
    let Some(head) = head else {
        return Ok(None);
    };
    let head_prompt = head.text.clone();
    let head_id = extract_head_id(&head_prompt);
    let reason = if has_auto {
        "active `agent:queue auto` still has a ready head prompt after a clean closeout"
    } else {
        "active persisted `agent:queue` (queue_active: true) still has a ready head prompt after a clean closeout"
    }
    .to_string();
    Ok(Some(QueueContinuation {
        reason,
        head_id,
        head_prompt,
    }))
}

/// The set of active backlog ids (lowercase) that are NOT agent-drainable
/// (`#qcontdrain`): `[operator-verify]` items only. `[clean-session]` is drainable
/// everywhere now — the in-session `/loop` drains it IN PLACE rather than deferring
/// to a (possibly-stalled) supervisor, so live editor-IPC state no longer gates the
/// deferred set. Used to compute queue continuation over the drainable head set only.
///
/// `pub(crate)` so `session_check`'s queue-head guards reuse the SAME deferred set
/// (`#goqueuestall`): a deferred head must not trip the "runnable head remained /
/// no-response reap-only closeout" guards, exactly as it is excluded here.
///
/// Pure (content-only). The supervisor still force-`/clear`s before a
/// `[clean-session]` head (`#cleandrainsup`, see [`head_requires_clean_session`]),
/// but that decision is independent of the drainable set computed here.
pub(crate) fn deferred_backlog_ids(content: &str) -> std::collections::HashSet<String> {
    let mut deferred = std::collections::HashSet::new();
    let Ok(components) = crate::component::parse(content) else {
        return deferred;
    };
    // `#qcontdrain` (operator override of `#goqueuestall`/`#cleandrainsup`/`#freshgrant`):
    // the in-session `/loop` drains `[clean-session]` heads IN PLACE instead of
    // deferring to the supervisor. Deferring stranded the queue whenever the
    // supervisor idle-watch was itself stalled (the live failure this fixes), so
    // `queue_continuation_required` must stay true while any non-`[operator-verify]`
    // head remains. Both the in-loop and supervisor drain paths defer ONLY
    // `[operator-verify]` (which genuinely needs a human); `[clean-session]` is
    // always drainable.
    for comp in &components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        for (id, ctx) in crate::pending::active_item_execution_contexts(body) {
            if ctx.operator_verify_required {
                // `[operator-verify]` genuinely needs a human — never drainable by
                // any agent scope.
                deferred.insert(id.to_ascii_lowercase());
            }
        }
    }
    deferred
}

/// Active backlog ids (lowercase) carrying `[clean-session]` — heads that ask for a
/// fresh agent context. The supervisor idle-watch force-`/clear`s before dispatching
/// such a head (`#cleandrainsup`) so it runs in a clean session even when the global
/// `agent_doc_queue_context_reset` opt-in is off.
pub fn clean_session_backlog_ids(content: &str) -> std::collections::HashSet<String> {
    let mut ids = std::collections::HashSet::new();
    let Ok(components) = crate::component::parse(content) else {
        return ids;
    };
    for comp in &components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        for (id, ctx) in crate::pending::active_item_execution_contexts(body) {
            if ctx.clean_session_required {
                ids.insert(id.to_ascii_lowercase());
            }
        }
    }
    ids
}

/// Whether the active queue `head` (an `#id` or raw prompt text) maps to a
/// `[clean-session]` backlog item (`#cleandrainsup`). The supervisor idle-watch uses
/// this to force a context `/clear` before dispatching the head, independent of the
/// global `agent_doc_queue_context_reset` opt-in.
pub fn head_requires_clean_session(file: &Path, head: &str) -> bool {
    let Ok(content) = std::fs::read_to_string(file) else {
        return false;
    };
    head_requires_clean_session_in(&content, head)
}

/// Pure core of [`head_requires_clean_session`] — testable without a file.
pub fn head_requires_clean_session_in(content: &str, head: &str) -> bool {
    let ids = clean_session_backlog_ids(content);
    if ids.is_empty() {
        return false;
    }
    let id = extract_head_id(head)
        .map(|i| i.to_ascii_lowercase())
        .unwrap_or_else(|| head.trim().to_ascii_lowercase());
    ids.contains(&id)
}

/// Count of active queue head prompts whose backlog id is deferred
/// (`#goqueuestall`). Used by `session-check` to surface a "queue idle: N head(s)
/// deferred" note when continuation is not required because every remaining head
/// is undrainable in the current session type.
pub fn deferred_head_count(file: &Path) -> usize {
    let Ok(content) = std::fs::read_to_string(file) else {
        return 0;
    };
    let Ok(components) = crate::component::parse(&content) else {
        return 0;
    };
    let Some(queue_component) = components.iter().find(|c| c.name == "queue") else {
        return 0;
    };
    let body = &content[queue_component.open_end..queue_component.close_start];
    let Ok(entries) = crate::queue::parse(body) else {
        return 0;
    };
    let deferred_ids = deferred_backlog_ids(&content);
    entries
        .iter()
        .filter_map(|entry| match entry {
            crate::queue::QueueEntry::Prompt(prompt) => extract_head_id(&prompt.text),
            _ => None,
        })
        .filter(|id| deferred_ids.contains(&id.to_ascii_lowercase()))
        .count()
}

/// The live auto-queue continuation head of a document **string**, independent
/// of any snapshot/sidecar. Returns `Some(head_id_or_prompt)` when `content` has
/// an active queue (`queue_active: true`) whose head is a ready prompt — not a
/// stop fence or a future time gate — else `None`.
///
/// Unlike [`detect`], this performs no snapshot-edit comparison: callers that
/// already hold two explicit document strings use it to compare continuation
/// state across snapshot / HEAD / working without a sidecar round-trip. It is the
/// authoritative-side signal for closeout metadata-drift recovery
/// (`#recovery-drift-authoritative-side`): a live continuation present in HEAD
/// but absent (or re-headed) in a metadata-only local drift means HEAD is
/// authoritative, because legitimate consumption of a queue head always shows up
/// as response/content drift, never as metadata-only drift.
pub fn live_continuation_head(file: &Path, content: &str) -> Option<String> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let (fm, _) = crate::frontmatter::parse_for_file_with_context(content, file, &rc).ok()?;
    if fm.queue_active != Some(true) {
        return None;
    }
    let components = crate::component::parse(content).ok()?;
    let queue_component = components.iter().find(|c| c.name == "queue")?;
    let has_auto = crate::queue::has_auto_attr(&queue_component.attrs);
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = crate::queue::parse(body).ok()?;
    let activation = crate::queue::resolve_activation(&entries, has_auto, false, true);
    if !activation.active
        || crate::queue::has_stop_fence_at_head(&activation.entries_after)
        || crate::queue::time_gate_at_head(&activation.entries_after).is_some()
    {
        return None;
    }
    let head = crate::queue::first_prompt(&activation.entries_after)?;
    Some(extract_head_id(&head.text).unwrap_or_else(|| head.text.trim().to_string()))
}

/// Pick the first DRAINABLE, non-deferred head prompt from an active queue's
/// post-activation entries, applying the same `#goqueuestall` / `#goqstall2`
/// filtering [`detect`] uses: skip inert noise lines (a bulleted free-text
/// observation with no `#id`, directive verb, or question) and skip heads whose
/// backlog id is deferred (`[operator-verify]` only; `#qcontdrain`). Single source
/// of truth for "is there agent-drainable work at the queue head" so the supervisor
/// idle-watch dispatch and `session-check` continuation agree.
fn first_drainable_head<'a>(
    entries_after: &'a [crate::queue::QueueEntry],
    open_backlog_ids: Option<&std::collections::HashSet<String>>,
    deferred_ids: &std::collections::HashSet<String>,
) -> Option<&'a crate::queue::QueuePrompt> {
    entries_after.iter().find_map(|entry| match entry {
        crate::queue::QueueEntry::Prompt(prompt) => {
            if head_is_drainable(&prompt.text, open_backlog_ids, deferred_ids) {
                Some(prompt)
            } else {
                None
            }
        }
        _ => None,
    })
}

/// Whether a single queue head is agent-drainable this session (`#cleardrainsignal`).
///
/// - Inert noise (a bare observation with no `#id`/directive/question, or a fenced
///   console-output paste) is never drainable.
/// - An `#id` head is drainable only when the id is an **open** `agent:backlog`
///   item AND not deferred (`[operator-verify]` only; `#qcontdrain`). A `do [#id]`
///   head whose id is absent from the open
///   backlog (already `agent:done`, archived, or an orphaned ref) is a stale head
///   the strike/reap path owns — NOT a continuation target, so it must not keep the
///   go-mode drain alive. This makes continuation agree with the no-response queue
///   guard, which intersects the head set with the open backlog the same way.
/// - A free-text directive/question head (no `#id`) is drainable.
fn head_is_drainable(
    text: &str,
    open_backlog_ids: Option<&std::collections::HashSet<String>>,
    deferred_ids: &std::collections::HashSet<String>,
) -> bool {
    if !is_drainable_queue_head(text) {
        return false;
    }
    match extract_head_id(text) {
        Some(id) => {
            let norm = id.to_ascii_lowercase();
            if deferred_ids.contains(&norm) {
                return false;
            }
            // Backlog-driven (go-mode) queue: an `#id` head must be an OPEN backlog
            // item. A `do [#id]` whose id left the backlog (completed, archived, or
            // an orphaned ref like a removed-without-archive id) is stale — the
            // strike/reap path owns it, not the drain — so it must not keep the
            // go-mode loop alive. When the doc has NO backlog component at all, the
            // id-heads ARE the work themselves (free-form queue), so don't gate on
            // membership.
            match open_backlog_ids {
                Some(open) => open.contains(&norm),
                None => true,
            }
        }
        None => true,
    }
}

/// Open (`[ ]`/`[/]`, not `[x]`/done) `agent:backlog` ids from a document string,
/// lowercased. Mirrors `session_check::done_signals::open_backlog_ids` but reads
/// the caller's `content` so continuation and the no-response guard agree on the
/// same drainable id set (`#cleardrainsignal`). Returns `None` when the document
/// has no `agent:backlog` component (a free-form id-head queue is not backlog-driven,
/// so id-head drainability is not gated on backlog membership).
fn open_backlog_ids_from_content(content: &str) -> Option<std::collections::HashSet<String>> {
    let components = crate::component::parse(content).ok()?;
    let mut found_backlog = false;
    let mut ids = std::collections::HashSet::new();
    for comp in &components {
        if !agent_doc_core::component::is_backlog_component(&comp.name) {
            continue;
        }
        found_backlog = true;
        let body = &content[comp.open_end..comp.close_start];
        let (_, items, _) = crate::pending::parse_items(body);
        for item in items {
            if !item.is_done() && !item.id.is_empty() {
                ids.insert(item.id.to_ascii_lowercase());
            }
        }
    }
    found_backlog.then_some(ids)
}

/// Count of OPEN (`[ ]`/`[/]`, not `[x]`/done) items in the `agent:review`
/// component of `content`. A multi-phase task's phase that needs human/external
/// validation is routed to `agent:review` as a gated `[/]` item; counting the
/// open review items lets a closeout detect that a phase was routed to review
/// this cycle (`#mphaseloop`).
fn open_review_item_count(content: &str) -> usize {
    let Ok(components) = crate::component::parse(content) else {
        return 0;
    };
    components
        .iter()
        .find(|comp| agent_doc_core::component::is_review_component(&comp.name))
        .map(|comp| {
            let body = &content[comp.open_end..comp.close_start];
            let (_, items, _) = crate::pending::parse_items(body);
            items.into_iter().filter(|item| !item.is_done()).count()
        })
        .unwrap_or(0)
}

/// Whether `current` routed at least one new phase to `agent:review` relative to
/// `prior`: the committed document has MORE open `agent:review` items than the
/// pre-commit HEAD (`#mphaseloop`).
///
/// The multi-phase auto-loop policy (operator directive 2026-06-14) requires the
/// go-mode drain to treat "needs review" as NON-terminal — a phase moved to
/// review must NOT halt the queue; the drain emits the review item and advances
/// to the next drainable head. The closeout uses this to emit the
/// `drain_continue_after_review` proof when a review-routed cycle still owes a
/// drainable continuation, distinguishing it from a turn that completed or
/// genuinely blocked its phase.
pub fn review_phase_routed(prior: &str, current: &str) -> bool {
    open_review_item_count(current) > open_review_item_count(prior)
}

/// The live **drainable** continuation head of a document string.
///
/// Like [`live_continuation_head`] but returns `Some` only when the active queue
/// has a head the agent can actually drain in the current session — applying the
/// same drainability + deferred filtering as [`detect`] (without the snapshot-edit
/// comparison): a `[clean-session]` head stays drainable (`#qcontdrain`; the
/// supervisor additionally force-`/clear`s before dispatch), and only
/// `[operator-verify]` heads and inert noise lines (operator bug-report
/// observations with no `#id`/directive/question) are skipped. A queue whose only
/// remaining heads are `[operator-verify]`/noise returns `None`.
///
/// The supervisor idle-queue watch uses this (not the unfiltered
/// [`live_continuation_head`]) so it does not re-inject a no-op `/agent-doc` drain
/// trigger every idle boundary for a queue `session-check` already reports as
/// having no continuation required (#qchurn). It computes the same drainable set as
/// the in-session `drainable_head_count` (`#qcontdrain`: both defer only
/// `[operator-verify]`).
pub fn live_drainable_continuation_head(file: &Path, content: &str) -> Option<String> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let (fm, _) = crate::frontmatter::parse_for_file_with_context(content, file, &rc).ok()?;
    if fm.queue_active != Some(true) {
        return None;
    }
    let components = crate::component::parse(content).ok()?;
    let queue_component = components.iter().find(|c| c.name == "queue")?;
    let has_auto = crate::queue::has_auto_attr(&queue_component.attrs);
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = crate::queue::parse(body).ok()?;
    let activation = crate::queue::resolve_activation(&entries, has_auto, false, true);
    if !activation.active
        || crate::queue::has_stop_fence_at_head(&activation.entries_after)
        || crate::queue::time_gate_at_head(&activation.entries_after).is_some()
    {
        return None;
    }
    let open_backlog = open_backlog_ids_from_content(content);
    // `#qcontdrain`/`#cleandrainsup`: only `[operator-verify]` is deferred. The
    // supervisor force-`/clear`s before a `[clean-session]` head, but that decision
    // is made separately (`head_requires_clean_session`) and does not change the
    // drainable set.
    let deferred_ids = deferred_backlog_ids(content);
    let head = first_drainable_head(&activation.entries_after, open_backlog.as_ref(), &deferred_ids)?;
    Some(extract_head_id(&head.text).unwrap_or_else(|| head.text.trim().to_string()))
}

/// Count of agent-drainable heads in `content`'s active queue (`#cleardrainsignal`).
///
/// Applies the SAME `#goqueuestall` / `#goqstall2` filtering as
/// [`live_drainable_continuation_head`] / [`first_drainable_head`]: a head counts
/// only when it is a real directive/`#id`/question (not inert noise) AND its
/// backlog id is not deferred (`[operator-verify]` only; `#qcontdrain`:
/// `[clean-session]` drains in place). Returns 0 when the queue is inactive,
/// stop-fenced, time-gated, or every remaining head is deferred/noise.
///
/// Preflight surfaces this so the agent and the Claude Code auto-loop have an
/// authoritative "nothing is agent-drainable, do not loop" signal that does NOT
/// depend on the route-owned supervisor being on the latest binary — the same
/// drainability the supervisor idle-watch already enforces (#qchurn).
pub fn drainable_head_count(file: &Path, content: &str) -> usize {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let Ok((fm, _)) = crate::frontmatter::parse_for_file_with_context(content, file, &rc) else {
        return 0;
    };
    if fm.queue_active != Some(true) {
        return 0;
    }
    let Ok(components) = crate::component::parse(content) else {
        return 0;
    };
    let Some(queue_component) = components.iter().find(|c| c.name == "queue") else {
        return 0;
    };
    let has_auto = crate::queue::has_auto_attr(&queue_component.attrs);
    let body = &content[queue_component.open_end..queue_component.close_start];
    let Ok(entries) = crate::queue::parse(body) else {
        return 0;
    };
    let activation = crate::queue::resolve_activation(&entries, has_auto, false, true);
    if !activation.active
        || crate::queue::has_stop_fence_at_head(&activation.entries_after)
        || crate::queue::time_gate_at_head(&activation.entries_after).is_some()
    {
        return 0;
    }
    let open_backlog = open_backlog_ids_from_content(content);
    let deferred_ids = deferred_backlog_ids(content);
    activation
        .entries_after
        .iter()
        .filter(|entry| match entry {
            crate::queue::QueueEntry::Prompt(prompt) => {
                head_is_drainable(&prompt.text, open_backlog.as_ref(), &deferred_ids)
            }
            _ => false,
        })
        .count()
}

/// Recognized actionable directive verbs for go-mode drainability classification
/// (`#goqstall2`). Word-matched anywhere in a normalized head — conservative in the
/// SAFE direction: a head that looks even loosely directive stays drainable; only a
/// bare observation with no directive and no question is demoted to inert noise.
///
/// `#cleardrainsignal`: deliberately EXCLUDES words that are common nouns in
/// agent-doc's own bug reports and would false-positive a declarative observation
/// into a drainable directive (e.g. "document" — "this document has…", "the
/// document model" — appears in nearly every report; the rare imperative "document
/// the API" arrives as a `do [#id]` head instead). Keep this list to verbs that
/// read as imperatives, not nouns, in queue prose.
const QUEUE_DIRECTIVE_VERBS: &[&str] = &[
    "do", "fix", "run", "build", "install", "commit", "push", "implement", "add",
    "update", "investigate", "create", "make", "remove", "delete", "refactor",
    "review", "explain", "drive", "resume", "continue", "check", "verify", "write",
    "test", "debug", "diagnose", "merge", "publish", "release", "bump",
    "rebase", "revert", "rename", "move", "split", "extract", "wire", "land", "ship",
    "draft", "summarize", "answer", "respond", "apply", "enable", "disable", "gate",
];

/// Strip a queue head's leading list bullet, emoji-shortcode tokens
/// (`:pushpin:` / `:round_pushpin:`), and leading emoji glyphs / stray punctuation
/// so the classifier sees the actionable text. Keeps `#` and `[` (an `#id` / `[#id]`
/// can lead the line).
fn normalize_queue_head_text(text: &str) -> String {
    let mut s = text.trim();
    if let Some(rest) = s.strip_prefix('-') {
        s = rest.trim_start();
    }
    // Strip leading `:shortcode:` emoji tokens (e.g. `:pushpin:`), repeatedly.
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
    // Strip leading emoji glyphs / stray punctuation, but keep `#`/`[` so an id can
    // lead and `/` so a slash command (`/model sonnet`) stays recognizable.
    s.trim_start_matches(|c: char| !c.is_alphanumeric() && c != '#' && c != '[' && c != '/')
        .trim()
        .to_string()
}

/// Whether a queue `Prompt` head is auto-drainable in go-mode (`#goqstall2`).
///
/// A pre-materialized `## Queue` block can carry bulleted free-text lines that are
/// not actionable drain targets — pasted bug-report observations or stale console
/// evidence the agent already triaged into backlog items. Those churn no-op
/// closeouts because the continuation walk treats every `Prompt` as a ready head.
///
/// A head is drainable iff it carries a `#id` / `[#id]` (the
/// `[clean-session]`/`[operator-verify]` defer is applied separately by id), ends
/// with a question mark, or contains a recognized imperative directive verb.
/// Everything else (a bare observation) is inert **noise**: excluded from the
/// continuation head set and surfaced as `queue_stale_noise_lines`, never
/// auto-deleted (the live IPC supervisor races on direct queue edits).
pub(crate) fn is_drainable_queue_head(text: &str) -> bool {
    // `#cleardrainsignal`: a queue head carrying a fenced ``` code block is pasted
    // console-output / error-report evidence the operator already triaged into
    // backlog ids (e.g. `JB \`Run Agent Doc\` ... \`\`\`<log>\`\`\``), NOT a drain
    // target — even though it incidentally contains a directive word like "run".
    // Such blocks would otherwise churn no-op go-mode cycles forever. An `#id` head
    // never carries a fence, so this only demotes free-text paste blocks.
    if text.contains("```") {
        return false;
    }
    if extract_head_id(text).is_some() {
        return true;
    }
    let normalized = normalize_queue_head_text(text);
    if normalized.is_empty() {
        return false;
    }
    // A harness slash command (`/clear`, `/model sonnet`, `/code-review`) is a
    // drainable command head, submitted to the owner pane — not prose noise.
    if crate::queue_command::is_slash_command(&normalized) {
        return true;
    }
    if normalized.trim_end().ends_with('?') {
        return true;
    }
    let lowered = normalized.to_ascii_lowercase();
    lowered
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| QUEUE_DIRECTIVE_VERBS.contains(&word))
}

/// Count of active queue `Prompt` heads that are non-drainable **noise**
/// (`#goqstall2`): not a `#id` head, not a directive, not a question. Used by
/// `session-check` to surface a `queue_stale_noise_lines=N` diagnostic so the
/// operator can clear pasted bug-report / console-evidence lines that would
/// otherwise churn the go-mode drain.
pub fn queue_stale_noise_lines(file: &Path) -> usize {
    let Ok(content) = std::fs::read_to_string(file) else {
        return 0;
    };
    let Ok(components) = crate::component::parse(&content) else {
        return 0;
    };
    let Some(queue_component) = components.iter().find(|c| c.name == "queue") else {
        return 0;
    };
    let body = &content[queue_component.open_end..queue_component.close_start];
    let Ok(entries) = crate::queue::parse(body) else {
        return 0;
    };
    entries
        .iter()
        .filter(|entry| match entry {
            crate::queue::QueueEntry::Prompt(prompt) => !is_drainable_queue_head(&prompt.text),
            _ => false,
        })
        .count()
}

/// Extract the backlog `#id` from a queue prompt like `do [#id] ...` or `#id ...`.
fn extract_head_id(prompt: &str) -> Option<String> {
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

/// Durable on-disk proof that a closed-out document still owes an auto-queue
/// continuation. Survives missing Codex hook session state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuationMarker {
    pub file: String,
    pub head_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_id: Option<String>,
    pub created_at: u64,
    pub source_command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_head: Option<String>,
    /// The head prompt last surfaced to a Codex Stop hook as a continuation
    /// request. Lets the hook fail closed when a repeated stop sees the same,
    /// non-advancing head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_requested_head: Option<String>,
}

fn marker_path(file: &Path) -> Result<Option<PathBuf>> {
    let Some(root) = crate::fs_util::find_project_root(file) else {
        return Ok(None);
    };
    let hash = crate::snapshot::doc_hash(file)?;
    Ok(Some(
        root.join(".agent-doc/queue-continuations")
            .join(format!("{hash}.json")),
    ))
}

/// Reconcile the durable continuation marker for `file` after a successful
/// closeout: write it when a continuation is required, clear it otherwise
/// (queue drained, `auto` removed, `queue_active` false, or head advanced).
/// Best-effort and never fatal to closeout — a marker write/clear failure is
/// logged, not propagated.
pub fn reconcile_marker(file: &Path, source_command: &str) -> Option<QueueContinuation> {
    match detect(file) {
        Ok(Some(continuation)) => {
            if let Err(err) = write_marker(file, &continuation, source_command) {
                eprintln!(
                    "[queue-continuation] WARNING: failed to write continuation marker for {}: {}",
                    file.display(),
                    err
                );
            }
            Some(continuation)
        }
        Ok(None) => {
            if let Err(err) = clear_marker(file) {
                eprintln!(
                    "[queue-continuation] WARNING: failed to clear continuation marker for {}: {}",
                    file.display(),
                    err
                );
            }
            None
        }
        Err(err) => {
            eprintln!(
                "[queue-continuation] WARNING: continuation detect failed for {}: {}",
                file.display(),
                err
            );
            None
        }
    }
}

pub fn write_marker(
    file: &Path,
    continuation: &QueueContinuation,
    source_command: &str,
) -> Result<()> {
    let Some(path) = marker_path(file)? else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    // Preserve the last continuation request across reconciles so the Stop-hook
    // non-advancing-head guard still works after a re-detect.
    let last_requested_head = load_marker(file)?.and_then(|marker| marker.last_requested_head);
    let marker = ContinuationMarker {
        file: file.display().to_string(),
        head_prompt: continuation.head_prompt.clone(),
        head_id: continuation.head_id.clone(),
        created_at: now_secs(),
        source_command: source_command.to_string(),
        commit_head: head_oid(file),
        last_requested_head,
    };
    let json = serde_json::to_string_pretty(&marker).context("serialize continuation marker")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn clear_marker(file: &Path) -> Result<()> {
    let Some(path) = marker_path(file)? else {
        return Ok(());
    };
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("remove {}", path.display())),
    }
}

fn cooldown_marker_path(file: &Path) -> Result<Option<PathBuf>> {
    let Some(root) = crate::fs_util::find_project_root(file) else {
        return Ok(None);
    };
    let hash = crate::snapshot::doc_hash(file)?;
    Ok(Some(
        root.join(".agent-doc/queue-cooldowns")
            .join(format!("{hash}.json")),
    ))
}

pub fn write_clear_cooldown(file: &Path) -> Result<()> {
    let Some(path) = cooldown_marker_path(file)? else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let payload = serde_json::json!({
        "file": file.to_string_lossy(),
        "written_at": now_secs(),
    });
    let json = serde_json::to_string_pretty(&payload).context("serialize cooldown marker")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn clear_cooldown_marker(file: &Path) -> Result<()> {
    let Some(path) = cooldown_marker_path(file)? else {
        return Ok(());
    };
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("remove {}", path.display())),
    }
}

pub fn clear_cooldown_active(file: &Path) -> Result<bool> {
    let Some(path) = cooldown_marker_path(file)? else {
        return Ok(false);
    };
    match std::fs::read_to_string(&path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| format!("read {}", path.display())),
    }
}

/// A clear that an operator command deferred because the pane was busy under an
/// active auto-queue loop (`#autoloop-command-preemption` Phase 2b). The
/// supervisor idle-queue watch delivers `clear_command` at the next idle gap,
/// then resumes the loop. The record is the durable hand-off between the
/// `session clear` command path (which pauses + records) and the supervisor
/// (which delivers + resumes), so the two never need to share memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeferredOperatorClear {
    pub file: String,
    /// Harness-specific clear command text to submit into the pane (e.g.
    /// `/clear`), captured at defer time so the watch does not re-derive it.
    pub clear_command: String,
    pub written_at: u64,
}

fn deferred_clear_marker_path(file: &Path) -> Result<Option<PathBuf>> {
    let Some(root) = crate::fs_util::find_project_root(file) else {
        return Ok(None);
    };
    let hash = crate::snapshot::doc_hash(file)?;
    Ok(Some(
        root.join(".agent-doc/deferred-clears")
            .join(format!("{hash}.json")),
    ))
}

/// Record that a non-interrupting operator clear was deferred while the pane was
/// busy under an active auto-loop. Paired with [`clear_cooldown`] (which pauses
/// the loop); the watch delivers `clear_command` once the pane is idle, then
/// clears both markers to resume.
pub fn write_deferred_operator_clear(file: &Path, clear_command: &str) -> Result<()> {
    let Some(path) = deferred_clear_marker_path(file)? else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let payload = DeferredOperatorClear {
        file: file.to_string_lossy().into_owned(),
        clear_command: clear_command.to_string(),
        written_at: now_secs(),
    };
    let json = serde_json::to_string_pretty(&payload).context("serialize deferred clear marker")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Read the pending deferred operator clear for `file`, if any.
pub fn read_deferred_operator_clear(file: &Path) -> Result<Option<DeferredOperatorClear>> {
    let Some(path) = deferred_clear_marker_path(file)? else {
        return Ok(None);
    };
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(serde_json::from_str(&content).ok()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("read {}", path.display())),
    }
}

/// Remove the deferred-clear marker (after the watch delivers the clear, or when
/// the operator runs an explicit interrupt-clear that supersedes it).
pub fn clear_deferred_operator_clear_marker(file: &Path) -> Result<()> {
    let Some(path) = deferred_clear_marker_path(file)? else {
        return Ok(());
    };
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("remove {}", path.display())),
    }
}

pub fn load_marker(file: &Path) -> Result<Option<ContinuationMarker>> {
    let Some(path) = marker_path(file)? else {
        return Ok(None);
    };
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(serde_json::from_str(&content).ok()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("read {}", path.display())),
    }
}

/// Record that the head prompt was surfaced to a Codex Stop hook as a
/// continuation request, so a subsequent stop with the same head can fail
/// closed instead of looping. No-op when no marker exists.
pub fn record_requested_head(file: &Path, head_prompt: &str) -> Result<()> {
    let Some(mut marker) = load_marker(file)? else {
        return Ok(());
    };
    marker.last_requested_head = Some(head_prompt.to_string());
    let Some(path) = marker_path(file)? else {
        return Ok(());
    };
    let json = serde_json::to_string_pretty(&marker).context("serialize continuation marker")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Scan every project root for a durable continuation marker whose document
/// still requires continuation. Used by the Codex Stop hook when no tracked
/// in-memory session state is available. Returns the first still-valid
/// `(file, continuation, marker)`.
/// `#codex-stop-cross-doc-queue-continuation`: a durable Stop-hook marker is
/// owned by *some* document; the fallback must not instruct the current Codex
/// pane to run a document owned by ANOTHER live actor. A marker doc is foreign
/// when it has a live (non-Closed) authoritative actor bound to a pane other
/// than `current_pane`. Unknown / closed / unowned ownership is NOT foreign
/// (allowed — covers the safe-claim and same-session cases). `current_pane` of
/// `None` (no tmux context) disables the gate and preserves prior behavior.
fn is_foreign_owned_marker(root: &Path, doc: &Path, current_pane: &str) -> bool {
    match crate::project_controller::authoritative_actor_binding(root, doc) {
        Ok(Some(record))
            if record.state != crate::session_actor::ActorState::Closed
                && !record.pane_id.trim().is_empty() =>
        {
            record.pane_id != current_pane
        }
        _ => false,
    }
}

/// Find the first still-valid durable `agent:queue auto` continuation marker
/// across `roots`. `current_pane` is the tmux pane of the Codex session whose
/// Stop hook is asking; markers owned by a different live pane are skipped (the
/// scan continues) so the hook never tells pane A to run document B while B has
/// its own live owner (`#codex-stop-cross-doc-queue-continuation`).
pub fn pending_marker_continuation_for_roots(
    roots: &[PathBuf],
    current_pane: Option<&str>,
) -> Result<Option<(PathBuf, QueueContinuation, ContinuationMarker)>> {
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        let dir = root.join(".agent-doc/queue-continuations");
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err).with_context(|| format!("read {}", dir.display())),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(marker) = serde_json::from_str::<ContinuationMarker>(&content) else {
                continue;
            };
            let doc = PathBuf::from(&marker.file);
            if !seen.insert(doc.clone()) {
                continue;
            }
            // `#codex-stop-cross-doc-queue-continuation`: skip a marker owned by
            // another live actor (different pane) and keep scanning, so this
            // Codex pane is never told to run a foreign-owned document. Does NOT
            // remove the marker — it stays for that document's own owner.
            if let Some(current) = current_pane
                && is_foreign_owned_marker(root, &doc, current)
            {
                crate::ops_log::log_op(
                    &doc,
                    &format!(
                        "codex_stop_foreign_queue_marker_skip file={} current_pane={}",
                        doc.display(),
                        current
                    ),
                );
                continue;
            }
            // The marker is durable but advisory — re-confirm against the live
            // document so a stale marker (queue since drained / edited) never
            // forces a spurious continuation.
            match detect(&doc)? {
                Some(continuation) => return Ok(Some((doc, continuation, marker))),
                None => {
                    // Stale marker — clean it up so it cannot mislead later.
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
    Ok(None)
}

fn head_oid(file: &Path) -> Option<String> {
    let dir = file.parent()?;
    let output = std::process::Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let oid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!oid.is_empty()).then_some(oid)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#degraded-ipc-no-stall`: the shared no-stall guidance must name the
    /// degraded-transport circumstance and the exhaustive stop list so neither
    /// preflight nor session-check can drift the wording into licensing a stall.
    #[test]
    fn continuation_guidance_names_degraded_ipc_and_exhaustive_stop_list() {
        let g = CONTINUATION_NO_STALL_GUIDANCE;
        assert!(g.contains("file-IPC"), "must name the file-IPC fallback");
        assert!(
            g.contains("committed") && g.contains("session-check"),
            "must state the successful-closeout proof"
        );
        assert!(
            g.contains("failed closeout")
                && g.contains("session-check interruption")
                && g.contains("lint-gate"),
            "must enumerate the exhaustive stop list"
        );
        assert!(
            g.contains("NOT stop reasons"),
            "must say degraded IPC / stale supervisor / accretion are NOT stop reasons"
        );
    }

    fn write_doc(dir: &Path, prompts: &[&str], queue_active: bool, has_auto: bool) -> PathBuf {
        std::fs::create_dir_all(dir.join(".agent-doc/snapshots")).unwrap();
        let doc = dir.join("task.md");
        let queue: String = prompts.iter().map(|p| format!("- {p}\n")).collect();
        let auto = if has_auto { " auto" } else { "" };
        let content = format!(
            "---\nsession: sid\nagent_doc_format: template\nqueue_active: {queue_active}\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue{auto} -->\n{queue}<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();
        doc
    }

    #[test]
    fn deferred_operator_clear_marker_roundtrips_and_clears() {
        // The durable hand-off between the `session clear` defer path and the
        // supervisor watch (`#autoloop-command-preemption` Phase 2b).
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#a]"], true, true);

        assert!(
            read_deferred_operator_clear(&doc).unwrap().is_none(),
            "no marker before a defer"
        );

        write_deferred_operator_clear(&doc, "/clear").unwrap();
        let record = read_deferred_operator_clear(&doc)
            .unwrap()
            .expect("marker present after write");
        assert_eq!(record.clear_command, "/clear");
        assert!(record.file.contains("task.md"));

        clear_deferred_operator_clear_marker(&doc).unwrap();
        assert!(
            read_deferred_operator_clear(&doc).unwrap().is_none(),
            "marker dropped after delivery/supersede"
        );
        // Clearing an absent marker is a no-op, not an error.
        clear_deferred_operator_clear_marker(&doc).unwrap();
    }

    fn doc_with_backlog(queue_prompts: &[&str], backlog_items: &[&str]) -> String {
        let queue: String = queue_prompts.iter().map(|p| format!("- {p}\n")).collect();
        let backlog: String = backlog_items.iter().map(|b| format!("{b}\n")).collect();
        format!(
            "---\nsession: sid\nagent_doc_format: template\nqueue_active: true\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n\
## Backlog\n\n<!-- agent:backlog queue=sync -->\n{backlog}<!-- /agent:backlog -->\n\n\
## Queue\n\n<!-- agent:queue auto -->\n{queue}<!-- /agent:queue -->\n"
        )
    }

    #[test]
    fn deferred_backlog_ids_defers_only_operator_verify() {
        // #qcontdrain: ONLY [operator-verify] is deferred. [clean-session] is
        // always drainable now — the in-session /loop drains it in place rather
        // than deferring to a possibly-stalled supervisor — so live IPC state no
        // longer changes the deferred set (the function is now content-only).
        let content = doc_with_backlog(
            &["do [#a]", "do [#b]", "do [#c]"],
            &[
                "- [ ] [#a] [clean-session] needs quiet",
                "- [ ] [#b] [operator-verify] live drive",
                "- [ ] [#c] plain",
            ],
        );
        let deferred = deferred_backlog_ids(&content);
        assert!(!deferred.contains("a"), "clean-session drains in-loop (#qcontdrain)");
        assert!(deferred.contains("b"), "operator-verify always deferred");
        assert!(!deferred.contains("c"));
    }

    fn doc_with_review(review_items: &[&str]) -> String {
        let review: String = review_items.iter().map(|r| format!("{r}\n")).collect();
        format!(
            "---\nsession: sid\nagent_doc_format: template\nqueue_active: true\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n\
## Review\n\n<!-- agent:review -->\n{review}<!-- /agent:review -->\n\n\
## Queue\n\n<!-- agent:queue auto -->\n- do [#next]\n<!-- /agent:queue -->\n"
        )
    }

    #[test]
    fn review_phase_routed_detects_added_open_review_item() {
        // #mphaseloop: moving a phase to agent:review (a gated `[/]` item) is the
        // signal that this cycle routed-to-review rather than completed/blocked.
        let none = doc_with_review(&[]);
        let one = doc_with_review(&["- [/] [#p1] phase 1 needs live verify"]);
        assert!(
            review_phase_routed(&none, &one),
            "a newly-added open review item is a routed phase"
        );
        // No delta → not routed (idempotent re-commit must not re-fire the proof).
        assert!(!review_phase_routed(&one, &one));
        // Fewer open items (a review item reaped/completed) is not a routed phase.
        assert!(!review_phase_routed(&one, &none));
        // A `[x]` done item does not count as an open routed phase.
        let done = doc_with_review(&["- [x] [#p1] phase 1 reviewed"]);
        assert!(!review_phase_routed(&none, &done));
    }

    #[test]
    fn review_phase_routed_counts_only_review_component() {
        // A growing backlog/queue must NOT be misread as a routed review phase —
        // only the agent:review component's open-item delta counts.
        let prior = doc_with_review(&["- [/] [#p1] one"]);
        let more_reviews = doc_with_review(&["- [/] [#p1] one", "- [/] [#p2] two"]);
        assert!(review_phase_routed(&prior, &more_reviews));
        assert_eq!(open_review_item_count(&prior), 1);
        assert_eq!(open_review_item_count(&more_reviews), 2);
        assert_eq!(open_review_item_count(&doc_with_review(&[])), 0);
    }

    #[test]
    fn every_clean_session_head_drains() {
        // #qcontdrain: clean-session heads drain in-loop unconditionally (the
        // `#freshgrant` grant machinery is gone). Only operator-verify defers.
        let content = doc_with_backlog(
            &["do [#a]", "do [#b]", "do [#e]"],
            &[
                "- [ ] [#a] [clean-session] one",
                "- [ ] [#b] [operator-verify] live drive",
                "- [ ] [#e] [clean-session] two",
            ],
        );
        let deferred = deferred_backlog_ids(&content);
        assert!(!deferred.contains("a"), "clean-session drains in-loop");
        assert!(!deferred.contains("e"), "every clean-session head drains in-loop");
        assert!(
            deferred.contains("b"),
            "operator-verify is never drainable by any agent scope"
        );
    }

    #[test]
    fn head_requires_clean_session_maps_head_id_to_tag() {
        // #cleandrainsup: the idle-watch maps the active head (an `#id`) back to its
        // backlog item's `[clean-session]` tag to decide whether to force a `/clear`.
        let content = doc_with_backlog(
            &["do [#a]", "do [#b]"],
            &[
                "- [ ] [#a] [clean-session] needs quiet",
                "- [ ] [#b] plain drainable",
            ],
        );
        assert!(head_requires_clean_session_in(&content, "a"));
        assert!(head_requires_clean_session_in(&content, "do [#a]"));
        assert!(!head_requires_clean_session_in(&content, "b"));
        assert!(!head_requires_clean_session_in(&content, "nonexistent"));
    }

    #[test]
    fn detect_skips_deferred_head_and_continues_on_drainable() {
        // #goqueuestall: with no live IPC listener, the clean-session head drains
        // normally, so continuation lands on it (first drainable head).
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let content = doc_with_backlog(
            &["do [#b]", "do [#c]"],
            &[
                "- [ ] [#b] [operator-verify] live drive",
                "- [ ] [#c] plain drainable",
            ],
        );
        std::fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();
        // No socket → not live IPC. operator-verify (#b) is deferred regardless,
        // so continuation must land on the drainable #c head.
        let continuation = detect(&doc).unwrap().expect("drainable head remains");
        assert_eq!(continuation.head_id.as_deref(), Some("c"));
    }

    #[test]
    fn detect_none_when_only_deferred_heads_remain() {
        // #goqueuestall: when every remaining head is undrainable
        // (operator-verify), continuation is NOT required — this is the stall fix.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let content = doc_with_backlog(
            &["do [#b]", "do [#d]"],
            &[
                "- [ ] [#b] [operator-verify] live drive",
                "- [ ] [#d] [operator-verify] also live",
            ],
        );
        std::fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();
        assert!(
            detect(&doc).unwrap().is_none(),
            "all-deferred heads must not require continuation"
        );
        assert_eq!(deferred_head_count(&doc), 2);
    }

    #[test]
    fn drainable_head_count_zero_when_only_deferred_heads_remain() {
        // #cleardrainsignal: the preflight no-stall signal must read 0 drainable
        // heads when every remaining head is undrainable (operator-verify always),
        // so the agent / Claude Code auto-loop never loops a #qchurn no-op cycle.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let content = doc_with_backlog(
            &["do [#b]", "do [#d]"],
            &[
                "- [ ] [#b] [operator-verify] live drive",
                "- [ ] [#d] [operator-verify] also live",
            ],
        );
        std::fs::write(&doc, &content).unwrap();
        assert_eq!(drainable_head_count(&doc, &content), 0);
    }

    #[test]
    fn drainable_head_count_counts_only_real_drainable_heads() {
        // #cleardrainsignal: a deferred head + an inert noise observation are
        // excluded; only the genuine directive head counts.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
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
        std::fs::write(&doc, &content).unwrap();
        // #b deferred (operator-verify), the bare observation is inert noise,
        // only #c is a real drainable head.
        assert_eq!(drainable_head_count(&doc, &content), 1);
    }

    #[test]
    fn drainable_head_count_excludes_orphan_id_head_when_backlog_present() {
        // #cleardrainsignal: a `do [#id]` head whose id is absent from the open
        // backlog (completed/archived/orphan) is stale — it must not keep the
        // go-mode drain alive when the doc is backlog-driven.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let content = doc_with_backlog(
            &["do [#orphan]", "do [#c]"],
            &["- [ ] [#c] plain drainable"],
        );
        std::fs::write(&doc, &content).unwrap();
        // #orphan has no backlog item → excluded; only #c counts.
        assert_eq!(drainable_head_count(&doc, &content), 1);
    }

    #[test]
    fn drainable_head_count_free_form_id_queue_without_backlog_still_drains() {
        // #cleardrainsignal regression guard: when the doc has NO backlog component,
        // id-heads are the work themselves and must stay drainable.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#x]", "do [#y]"], true, true);
        let content = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(drainable_head_count(&doc, &content), 2);
    }

    #[test]
    fn is_drainable_queue_head_treats_fenced_paste_as_noise() {
        // #cleardrainsignal: a pasted bug-report/console block (fenced ```) is noise
        // even though it incidentally contains the directive word "run".
        let fenced = ":pushpin: JB `Run Agent Doc` should self-heal.\n```\n[route] target tmux session: 0\nError: dispatch blocked\n```";
        assert!(!is_drainable_queue_head(fenced));
        // A genuine inline directive (no fence) stays drainable.
        assert!(is_drainable_queue_head("run the full test suite"));
    }

    #[test]
    fn detect_returns_head_for_active_auto_queue() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &["do [#seopdp] next", "do [#third]"],
            true,
            true,
        );
        let continuation = detect(&doc).unwrap().expect("ready auto-queue head");
        assert_eq!(continuation.head_prompt, "do [#seopdp] next");
        assert_eq!(continuation.head_id.as_deref(), Some("seopdp"));
    }

    #[test]
    fn detect_returns_head_for_persisted_active_queue_without_auto() {
        // `#active-queue-persisted-no-continue`: a persisted-active queue
        // (queue_active: true) without the `auto` attribute still owes
        // continuation — `auto` is a start trigger only.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &["do [#persisted] next", "do [#third]"],
            true,
            false,
        );
        let continuation = detect(&doc).unwrap().expect("ready persisted-active head");
        assert_eq!(continuation.head_prompt, "do [#persisted] next");
        assert_eq!(continuation.head_id.as_deref(), Some("persisted"));
        assert!(
            continuation.reason.contains("persisted"),
            "persisted-active reason should name the persisted trigger, got: {}",
            continuation.reason
        );
    }

    #[test]
    fn detect_none_when_inactive_plain_queue() {
        // A queue without `auto` AND without `queue_active: true` must never
        // self-start — the `queue_active` guard fails first.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#x]"], false, false);
        assert!(detect(&doc).unwrap().is_none());
    }

    #[test]
    fn detect_none_when_queue_inactive() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#x]"], false, true);
        assert!(detect(&doc).unwrap().is_none());
    }

    #[test]
    fn detect_none_when_queue_drained() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &[], true, true);
        assert!(detect(&doc).unwrap().is_none());
    }

    #[test]
    fn detect_none_when_stop_fence_at_head() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["-- stop placeholder"], true, true);
        // Replace the queue body with a real stop fence at the head.
        let content = std::fs::read_to_string(&doc)
            .unwrap()
            .replace("- -- stop placeholder\n", "--- stop\n- do [#x]\n");
        std::fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();
        // A stop fence at the head must not force continuation.
        assert!(detect(&doc).unwrap().is_none());
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

    #[test]
    fn is_drainable_queue_head_classifies_directive_vs_noise() {
        // #goqstall2: `#id` heads, directives, and questions are drainable.
        assert!(is_drainable_queue_head(":round_pushpin: do [#fcc0]"));
        assert!(is_drainable_queue_head("- :pushpin: Fix the submit bug"));
        assert!(is_drainable_queue_head(
            "JB Run Agent Doc ... does not submit. Fix and add Simworld regression tests."
        ));
        assert!(is_drainable_queue_head(
            "JB Clear Exchange ... the content came back...from a stale HEAD?"
        ));
        assert!(is_drainable_queue_head("#bare-id continue the drain"));
        // Harness slash commands are drainable command heads, not prose noise.
        assert!(is_drainable_queue_head("- /model sonnet"));
        assert!(is_drainable_queue_head(":pushpin: /clear"));

        // Bare bug-report observations with no directive and no question → noise.
        assert!(!is_drainable_queue_head(
            "- I'm still seeing JB `File Cache Conflict` dialogs. There should be 0."
        ));
        assert!(!is_drainable_queue_head(
            ":pushpin: JB `Compact Exchange` has a partially uncommitted response."
        ));
        assert!(!is_drainable_queue_head("- "));
        assert!(!is_drainable_queue_head(":pushpin:"));

        // #cleardrainsignal: declarative observations whose only "verb" match is the
        // noun "document" are noise, not directives (these churned bugs2.md).
        assert!(!is_drainable_queue_head(
            "- This document has `agent:backlog priority queue`...but the backlog items are not being added to the `agent:queue`."
        ));
        assert!(!is_drainable_queue_head(
            ":pushpin: Ensure the markdown AST handles strings and code blocks. Tags within the blocks should be treated as content, not as part of the document tags."
        ));
    }

    #[test]
    fn detect_skips_noise_head_and_lands_on_drainable_directive() {
        // #goqstall2: a bare bug-report observation ahead of a real directive must
        // not be served as the continuation head — the drain lands on the directive.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &[
                "I'm still seeing JB File Cache Conflict dialogs. There should be 0.",
                "Fix the submit bug and add coverage",
            ],
            true,
            true,
        );
        let continuation = detect(&doc).unwrap().expect("a drainable head remains");
        assert_eq!(continuation.head_prompt, "Fix the submit bug and add coverage");
        assert_eq!(queue_stale_noise_lines(&doc), 1);
    }

    #[test]
    fn detect_quiesces_when_only_noise_heads_remain() {
        // #goqstall2: a queue of only bare bug-report observations is NOT a stall —
        // continuation is not required and the lines are counted as stale noise.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &[
                "I'm still seeing JB File Cache Conflict dialogs. There should be 0.",
                "JB Compact Exchange has a partially uncommitted response.",
            ],
            true,
            true,
        );
        assert!(
            detect(&doc).unwrap().is_none(),
            "only-noise queue must not require continuation"
        );
        assert_eq!(queue_stale_noise_lines(&doc), 2);
    }

    #[test]
    fn live_drainable_head_skips_noise_and_lands_on_directive() {
        // #qchurn: the supervisor idle-watch dispatch head must skip inert noise
        // lines and land on a real directive — matching detect(), so the watch does
        // not churn no-op /agent-doc cycles when the only ready head is noise.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &[
                "I'm still seeing JB File Cache Conflict dialogs. There should be 0.",
                "Fix the submit bug and add coverage",
            ],
            true,
            true,
        );
        let content = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            live_drainable_continuation_head(&doc, &content).as_deref(),
            Some("Fix the submit bug and add coverage"),
        );
    }

    #[test]
    fn live_drainable_head_none_when_only_noise() {
        // #qchurn: a queue of only bare bug-report observations yields NO drainable
        // idle-watch head, so the supervisor stops re-dispatching. The unfiltered
        // live_continuation_head still returns the first head — proving the new
        // filter is what quiesces the churn.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &[
                "I'm still seeing JB File Cache Conflict dialogs. There should be 0.",
                "JB Compact Exchange has a partially uncommitted response.",
            ],
            true,
            true,
        );
        let content = std::fs::read_to_string(&doc).unwrap();
        assert!(
            live_drainable_continuation_head(&doc, &content).is_none(),
            "only-noise queue must not yield a drainable idle-watch head"
        );
        assert!(
            live_continuation_head(&doc, &content).is_some(),
            "unfiltered head still returns the first noise line (the old churn source)"
        );
    }

    #[test]
    fn reconcile_marker_writes_then_clears() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#seopdp]"], true, true);

        // Active continuation → marker written.
        let continuation = reconcile_marker(&doc, "commit").expect("continuation required");
        assert_eq!(continuation.head_prompt, "do [#seopdp]");
        let marker = load_marker(&doc).unwrap().expect("marker persisted");
        assert_eq!(marker.head_prompt, "do [#seopdp]");
        assert_eq!(marker.source_command, "commit");

        // Drain the queue (queue_active flips false) → marker cleared.
        let _ = write_doc(dir.path(), &["do [#seopdp]"], false, true);
        assert!(reconcile_marker(&doc, "commit").is_none());
        assert!(load_marker(&doc).unwrap().is_none());
    }

    #[test]
    fn pending_marker_for_roots_finds_then_prunes_stale() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let doc = write_doc(&root, &["do [#seopdp]"], true, true);
        reconcile_marker(&doc, "commit").expect("marker written");

        // The marker is found and re-confirmed against the live document.
        let found = pending_marker_continuation_for_roots(&[root.clone()], None)
            .unwrap()
            .expect("durable continuation found");
        assert_eq!(found.0, doc);
        assert_eq!(found.1.head_prompt, "do [#seopdp]");

        // Drain the queue but leave the marker file on disk (stale).
        let _ = write_doc(&root, &["do [#seopdp]"], false, true);
        let path = marker_path(&doc).unwrap().unwrap();
        assert!(path.exists(), "stale marker still on disk before scan");
        // Scan re-confirms against the document, finds it no longer owes
        // continuation, returns None, and prunes the stale marker.
        assert!(
            pending_marker_continuation_for_roots(&[root.clone()], None)
                .unwrap()
                .is_none()
        );
        assert!(!path.exists(), "stale marker pruned during scan");
    }

    // `#codex-stop-cross-doc-queue-continuation`: a marker for a document owned
    // by another live actor's pane must be skipped (not driven) when the current
    // Codex pane differs; a same-pane / unowned marker is still returned.
    #[test]
    fn pending_marker_skips_foreign_owned_then_finds_same_pane() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        // Foreign doc: owned by a live actor on pane %70.
        let foreign = write_doc(&root, &["do [#foreign]"], true, true);
        reconcile_marker(&foreign, "commit").expect("foreign marker written");
        crate::session_actor::project_binding_in(
            &root,
            &foreign.to_string_lossy(),
            "foreign-session",
            "%70",
            "@1",
            "test",
            "foreign_owner",
        )
        .unwrap();

        // From pane %74, the foreign-owned marker must be skipped → None.
        assert!(
            pending_marker_continuation_for_roots(&[root.clone()], Some("%74"))
                .unwrap()
                .is_none(),
            "foreign-owned marker (pane %70) must be skipped from pane %74"
        );
        // The foreign marker must NOT be pruned — it belongs to its own owner.
        assert!(
            marker_path(&foreign).unwrap().unwrap().exists(),
            "foreign marker must survive the skip (not stale)"
        );

        // The foreign doc's OWN pane (%70) still drives its marker.
        let owned = pending_marker_continuation_for_roots(&[root.clone()], Some("%70"))
            .unwrap()
            .expect("same-pane owner drives its own marker");
        assert_eq!(owned.0, foreign);

        // Unknown pane context (None) preserves prior behavior — returns it.
        assert!(
            pending_marker_continuation_for_roots(&[root.clone()], None)
                .unwrap()
                .is_some(),
            "None current_pane disables the gate (prior behavior)"
        );
    }

    // `#codex-stop-cross-doc-queue-continuation` (scan ordering): a foreign-owned
    // marker scanned BEFORE a valid same-pane marker must be skipped while the
    // scan continues to return the later valid marker — never the foreign one.
    #[test]
    fn pending_marker_scan_continues_past_foreign_to_valid() {
        let foreign_dir = tempfile::tempdir().unwrap();
        let valid_dir = tempfile::tempdir().unwrap();
        let foreign_root = foreign_dir.path().to_path_buf();
        let valid_root = valid_dir.path().to_path_buf();

        // Foreign doc (scanned first): owned by a live actor on pane %70.
        let foreign = write_doc(&foreign_root, &["do [#foreign]"], true, true);
        reconcile_marker(&foreign, "commit").expect("foreign marker written");
        crate::session_actor::project_binding_in(
            &foreign_root,
            &foreign.to_string_lossy(),
            "foreign-session",
            "%70",
            "@1",
            "test",
            "foreign_owner",
        )
        .unwrap();

        // Valid doc (scanned second): owned by the current pane %74.
        let valid = write_doc(&valid_root, &["do [#valid]"], true, true);
        reconcile_marker(&valid, "commit").expect("valid marker written");
        crate::session_actor::project_binding_in(
            &valid_root,
            &valid.to_string_lossy(),
            "current-session",
            "%74",
            "@1",
            "test",
            "current_owner",
        )
        .unwrap();

        // From pane %74, the foreign root is scanned first; its %70-owned marker
        // is skipped and the scan continues to the %74-owned valid marker.
        let found = pending_marker_continuation_for_roots(
            &[foreign_root.clone(), valid_root.clone()],
            Some("%74"),
        )
        .unwrap()
        .expect("scan must continue past foreign marker to the valid one");
        assert_eq!(
            found.0, valid,
            "must return the same-pane valid doc, not foreign"
        );
        assert_eq!(found.1.head_prompt, "do [#valid]");

        // The skipped foreign marker must survive for its own owner.
        assert!(
            marker_path(&foreign).unwrap().unwrap().exists(),
            "foreign marker must survive the skip (belongs to its own owner)"
        );
    }

    #[test]
    fn record_requested_head_persists_for_nonadvancing_guard() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#seopdp]"], true, true);
        reconcile_marker(&doc, "commit").expect("marker written");
        record_requested_head(&doc, "do [#seopdp]").unwrap();
        let marker = load_marker(&doc).unwrap().unwrap();
        assert_eq!(marker.last_requested_head.as_deref(), Some("do [#seopdp]"));
        // A re-detect/reconcile preserves the requested head.
        reconcile_marker(&doc, "commit");
        let marker = load_marker(&doc).unwrap().unwrap();
        assert_eq!(marker.last_requested_head.as_deref(), Some("do [#seopdp]"));
    }
}
