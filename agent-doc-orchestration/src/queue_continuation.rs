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
/// `session-check ok` through a **proven file-IPC editor fallback** (socket ack
/// timeouts / a stale or wedged route-owned supervisor) is a *successful*
/// closeout — the in-session loop does not depend on the socket itself. The
/// agent must not invent a stop reason from the degraded-but-proven transport.
/// Unproven IPC (`retry_without_disk_write`, missing response proof, no ack) or
/// a direct session-document disk fallback is different: it stops the loop until
/// the editor/CRDT path is retried and proven. The normal closeout states that
/// stop the loop are a FAILED closeout, an unproven IPC/delivery retry
/// condition, a `session-check` interruption, or a `lint-gate` block. Degraded
/// / file-IPC-fallback IPC after proof, high session-accretion, a
/// `semantic_completion_match` warning, and a `[clean-session]` head wanting
/// "fresh context" are NOT stop reasons. A stale-binary supervisor is not a
/// passive keep-going condition either: recycle/yield at the boundary, then
/// continue on the fresh binary.
pub const CONTINUATION_NO_STALL_GUIDANCE: &str = "queue continuation required — keep draining after a proven closeout. A closeout that reached committed + session-check ok is successful even via a proven file-IPC editor fallback (socket degraded): the in-session loop does not depend on the socket itself. IPC timeout, missing response proof, recovery=retry_without_disk_write, or recovery=direct_write_fallback are not successful closeouts for an active editor buffer; retry the editor/CRDT path instead of using a direct session-document disk write. Only a failed closeout, unproven IPC/delivery retry condition, session-check interruption, or lint-gate block stops the loop. Degraded IPC after proof, high session-accretion, and semantic_completion_match warnings are NOT stop reasons. A stale-binary supervisor is a recycle/yield concern: follow recycle-yield or stale_install guidance so the supervisor recycles, then continue the drain on the fresh binary.";

/// `#wd40` / `#staleloop-recycle-restart` — guidance surfaced when the route-owned
/// supervisor is running a stale binary and has asked the in-session loop to YIELD
/// one inter-item boundary so the supervisor can `execve`-hot-reload onto the
/// freshly-installed binary.
///
/// This is a *temporary, intentional* drop of `queue_continuation_required`, NOT a
/// drained queue or a stop reason: end the current turn cleanly (do not re-trigger
/// `agent-doc <FILE>`); the resulting idle boundary lets the stale supervisor
/// recycle, and the drain resumes automatically on the fresh binary. Releasing the
/// drain-owner lease (`agent-doc drain-claim <FILE> --release`) hands the drain
/// back to the freshly-recycled supervisor immediately instead of waiting for the
/// lease TTL.
pub const RECYCLE_YIELD_GUIDANCE: &str = "supervisor recycle pending — the route-owned supervisor is running a STALE agent-doc binary and has asked this loop to YIELD one boundary so it can hot-reload onto the freshly-installed binary. This is intentional and temporary, NOT a drained queue or a stop reason. End this turn cleanly (do NOT re-trigger agent-doc <FILE>); the idle boundary lets the stale supervisor execve-recycle and the drain resumes automatically on the fresh binary. Optionally run `agent-doc drain-claim <FILE> --release` to hand the drain back to the recycled supervisor immediately rather than waiting for the lease TTL. Do not run `make install` / `admin recycle` by hand — the supervisor now automates the recycle once you yield.";

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
    // `#qpausego` note: a controller `admin queue pause` does NOT short-circuit
    // continuation here. The pause suppresses the *unattended* supervisor
    // idle-watch auto-injection (see `start/idle_watch.rs`), but the attended
    // in-session `/loop` — and `session-check` / the codex-stop continuation
    // gate that consult this detector — must keep draining real queue work. A
    // pause stalling the in-session loop strands genuine drainable backlog (the
    // operator-rejected over-reach); `queue: stop` / `--- stop` is the in-session
    // stop control.
    // `#wd40` / `#staleloop-recycle-restart`: when the route-owned supervisor is
    // stale and has asked the in-session loop to yield one boundary so it can
    // hot-reload onto a freshly-installed binary, report no continuation so the
    // loop ends its turn. The idle boundary lets the `execve` recycle fire; the
    // fresh supervisor clears the request and the drain resumes. This is a
    // *temporary* yield (the request is short-TTL and cleared post-recycle), not a
    // drained queue — surfaces are expected to print [`RECYCLE_YIELD_GUIDANCE`].
    // The supervisor's OWN idle-watch drain uses `live_drainable_continuation_head`
    // (not this), so it is unaffected and resumes the drain after recycling.
    if crate::recycle_yield::recycle_yield_pending(file) {
        return Ok(None);
    }
    let content = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    detect_in_content(file, &content)
}

/// Whether the document's effective controller queue-control state is `paused`
/// (`#qpausego`).
///
/// An accepted `agent-doc admin queue pause <FILE>` records a durable
/// `queue_controls` row that the controller *dispatch* RPC already honors
/// (`load_effective_queue_control_from_db` → `failed_stage=queue_paused`). But
/// the supervisor idle-watch injects `agent-doc <FILE>` triggers straight into
/// the pane — bypassing that RPC — and this continuation signal was computed from
/// the document alone, so a `go`-mode auto-queue kept re-dispatching after an
/// accepted pause. Resolving the controller pause here lets both the idle-watch
/// drain decision and `preflight` defer to an accepted pause even for `go`-mode
/// queues. `resume`/`drain` are not `paused`, so they do not block here (the
/// controller owns draining).
///
/// Best-effort and read-only: returns `false` (not paused) when the project root
/// or controller state DB cannot be resolved/opened, so a missing control plane
/// never wedges an otherwise-active queue. A non-absent open/query error is
/// logged to stderr (never silently swallowed) and treated as not-paused so a
/// transient DB hiccup cannot strand the drain.
pub fn document_queue_controller_paused(file: &Path) -> bool {
    document_queue_controller_pause_reason(file).is_some()
}

/// The effective controller pause reason for `file` when (and only when) the
/// queue-control state is `paused` (`#qpausego` / `#qpausemix`).
///
/// Returns `Some(reason)` when an accepted `admin queue pause` is the effective
/// control state — `reason` is the operator/controller-recorded pause reason, or
/// an empty string when the pause carried none. Returns `None` when the queue is
/// not controller-paused (or the control plane / state DB cannot be resolved).
///
/// Surfacing the reason is what resolves the operator-perceived "mixed signal"
/// (`queue_paused: true` alongside `queue_continuation_required: true`): the
/// reason and the pause-aware [`continuation_guidance`] preamble let the agent
/// see *why* the queue was paused and that the pause only suppresses the
/// unattended supervisor idle-watch, instead of guessing whether the pause is
/// operator intent or transient drain-coordination state. Same best-effort,
/// read-only error handling as [`document_queue_controller_paused`].
pub fn document_queue_controller_pause_reason(file: &Path) -> Option<String> {
    let root = crate::snapshot::find_project_root(file)?;
    let db_path = agent_doc_sqlite::state_store::state_db_path(&root);
    if !db_path.exists() {
        // No control plane has ever run for this project: nothing can be paused.
        return None;
    }
    let canonical = file.canonicalize().ok()?;
    let document_id = canonical.to_string_lossy().to_string();
    let conn = match agent_doc_sqlite::state_store::open_state_db(&root) {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!(
                "[agent-doc] queue_continuation: failed to open controller state DB at {} ({err:#}) — treating queue as not controller-paused",
                root.display()
            );
            return None;
        }
    };
    match agent_doc_sqlite::state_store::load_effective_queue_control_from_db(
        &conn,
        &document_id,
        &root.to_string_lossy(),
    ) {
        Ok(control) => control.and_then(|control| {
            (control.state == "paused").then(|| control.reason.unwrap_or_default())
        }),
        Err(err) => {
            eprintln!(
                "[agent-doc] queue_continuation: failed to load controller queue control for {} ({err:#}) — treating queue as not controller-paused",
                file.display()
            );
            None
        }
    }
}

/// Compose the binary-authoritative queue-continuation guidance, resolving the
/// `queue_paused` + `queue_continuation_required` "mixed signal" (`#qpausemix`).
///
/// When the queue is not controller-paused (`pause_reason == None`), this returns
/// the base [`CONTINUATION_NO_STALL_GUIDANCE`] verbatim. When an accepted
/// `admin queue pause` is in effect, it prepends a preamble that explicitly
/// states the two signals are NOT contradictory — the controller pause suppresses
/// only the *unattended* supervisor idle-watch auto-injection, while the attended
/// in-session loop remains the legitimate single-owner drain — and surfaces the
/// recorded pause reason. This is the single source consumed by both preflight
/// JSON (`queue_continuation_guidance`) and `session-check` stdout so they stay
/// in agreement.
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

fn detect_in_content(file: &Path, content: &str) -> Result<Option<QueueContinuation>> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let (fm, _) = crate::frontmatter::parse_for_file_with_context(content, file, &rc)?;
    if fm.queue_active != Some(true) {
        return Ok(None);
    }
    let components = agent_doc_element::element::parse(content)?;
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
        && let Ok(snapshot_components) = agent_doc_element::element::parse(&snapshot_content)
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
    let preset_supplies_directive = queue_component.attrs.contains_key("preset");
    let head = first_drainable_head(
        &activation.entries_after,
        open_backlog.as_ref(),
        &deferred_ids,
        preset_supplies_directive,
    );
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
    deferred_backlog_ids_scoped(content, DrainScope::InSessionLoop)
}

/// The set of active backlog ids (lowercase) that are NOT drainable by the
/// SUPERVISOR clear-and-continue idle-watch drain (`#qfocsup`): `[operator-verify]`
/// items only. Unlike the in-session [`deferred_backlog_ids`], a `[focused-cycle]`
/// id is NOT in this set — the supervisor force-`/clear`s and re-dispatches it to a
/// fresh context (see [`head_requires_context_reset`]), so the queue keeps draining
/// instead of stranding idle. The supervisor head picker
/// [`live_drainable_continuation_head`] uses this scope; the in-session loop and
/// `session-check` guards use the narrower in-session scope.
pub(crate) fn supervisor_deferred_backlog_ids(content: &str) -> std::collections::HashSet<String> {
    deferred_backlog_ids_scoped(content, DrainScope::Supervisor)
}

/// Drain scope for computing the deferred (non-drainable) backlog id set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrainScope {
    /// In-session `/loop`: defers `[operator-verify]` AND `[focused-cycle]` — the
    /// current accreted session cannot give a `[focused-cycle]` item the fresh
    /// context it requires, so the loop yields it to the supervisor.
    InSessionLoop,
    /// Supervisor idle-watch clear-and-continue: defers `[operator-verify]` only.
    /// `[focused-cycle]` is drained via a forced `/clear` + re-dispatch (`#qfocsup`).
    Supervisor,
}

/// Shared core of the deferred-id computation, parameterized by drain scope.
///
/// `#qcontdrain` (operator override of `#goqueuestall`/`#cleandrainsup`/`#freshgrant`):
/// the in-session `/loop` drains `[clean-session]` heads IN PLACE instead of
/// deferring to the supervisor. `#qfocsup` (operator directive): a `[focused-cycle]`
/// head is deferred by the in-session loop but DRAINED by the supervisor's
/// clear-and-continue path, so it never strands the queue idle. Drainability is
/// owned ENTIRELY by these tags — the agent must never re-derive non-drainability
/// from item prose.
fn deferred_backlog_ids_scoped(
    content: &str,
    scope: DrainScope,
) -> std::collections::HashSet<String> {
    let mut deferred = std::collections::HashSet::new();
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return deferred;
    };
    for comp in &components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        for (id, ctx) in crate::pending::active_item_execution_contexts(body) {
            let undrainable = match scope {
                // `[operator-verify]` needs a human; `[focused-cycle]` needs a
                // freshly-cleared cycle the in-session loop cannot provide.
                DrainScope::InSessionLoop => ctx.loop_undrainable(),
                // The supervisor can clear-and-continue, so only `[operator-verify]`
                // is undrainable for it.
                DrainScope::Supervisor => ctx.supervisor_undrainable(),
            };
            if undrainable {
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
    let Ok(components) = agent_doc_element::element::parse(content) else {
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

/// Active backlog ids (lowercase) carrying `[focused-cycle]` — heads that must be
/// yielded by the in-session loop but drained by the supervisor after a forced
/// context reset (`#qfocsup`).
pub fn focused_cycle_backlog_ids(content: &str) -> std::collections::HashSet<String> {
    let mut ids = std::collections::HashSet::new();
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return ids;
    };
    for comp in &components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        for (id, ctx) in crate::pending::active_item_execution_contexts(body) {
            if ctx.focused_cycle_required {
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

/// Whether the active queue `head` maps to a `[focused-cycle]` backlog item
/// (`#qfocsup`). Such heads are supervisor-drainable only after a forced context
/// reset, and the ops log needs the focused-cycle-specific reason instead of the
/// clean-session reason.
pub fn head_requires_focused_cycle(file: &Path, head: &str) -> bool {
    let Ok(content) = std::fs::read_to_string(file) else {
        return false;
    };
    head_requires_focused_cycle_in(&content, head)
}

/// Pure core of [`head_requires_focused_cycle`] — testable without a file.
pub fn head_requires_focused_cycle_in(content: &str, head: &str) -> bool {
    let ids = focused_cycle_backlog_ids(content);
    if ids.is_empty() {
        return false;
    }
    let id = extract_head_id(head)
        .map(|i| i.to_ascii_lowercase())
        .unwrap_or_else(|| head.trim().to_ascii_lowercase());
    ids.contains(&id)
}

/// Active backlog ids (lowercase) for which the SUPERVISOR must force a context
/// `/clear` before dispatching the head: `[clean-session]` OR `[focused-cycle]`
/// (`#qfocsup`). A `[focused-cycle]` item needs a genuinely fresh context — that is
/// precisely why it is not in-session-drainable — so the supervisor clears before
/// re-dispatching it, exactly as it does for `[clean-session]`.
pub fn context_reset_backlog_ids(content: &str) -> std::collections::HashSet<String> {
    let mut ids = std::collections::HashSet::new();
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return ids;
    };
    for comp in &components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        for (id, ctx) in crate::pending::active_item_execution_contexts(body) {
            if ctx.clean_session_required || ctx.focused_cycle_required {
                ids.insert(id.to_ascii_lowercase());
            }
        }
    }
    ids
}

/// Whether the active queue `head` maps to a backlog item that requires the
/// supervisor to force a context `/clear` before dispatch — `[clean-session]` OR
/// `[focused-cycle]` (`#qfocsup`). Superset of [`head_requires_clean_session`].
pub fn head_requires_context_reset(file: &Path, head: &str) -> bool {
    let Ok(content) = std::fs::read_to_string(file) else {
        return false;
    };
    head_requires_context_reset_in(&content, head)
}

/// Pure core of [`head_requires_context_reset`] — testable without a file.
pub fn head_requires_context_reset_in(content: &str, head: &str) -> bool {
    let ids = context_reset_backlog_ids(content);
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
    let Ok(components) = agent_doc_element::element::parse(&content) else {
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
    let components = agent_doc_element::element::parse(content).ok()?;
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
    preset_supplies_directive: bool,
) -> Option<&'a crate::queue::QueuePrompt> {
    entries_after.iter().find_map(|entry| match entry {
        crate::queue::QueueEntry::Prompt(prompt) => {
            if head_is_drainable(
                &prompt.text,
                open_backlog_ids,
                deferred_ids,
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

/// Whether a single queue head is agent-drainable this session (`#cleardrainsignal`).
///
/// - Inert noise (structural/log artifacts such as pasted console output or
///   spliced agent response fragments) is never drainable.
/// - An `#id` head is drainable only when the id is an **open** `agent:backlog`
///   item AND not deferred (`[operator-verify]` only; `#qcontdrain`). A `do [#id]`
///   head whose id is absent from the open
///   backlog (already `agent:done`, archived, or an orphaned ref) is a stale head
///   the strike/reap path owns — NOT a continuation target, so it must not keep the
///   go-mode drain alive. This makes continuation agree with the no-response queue
///   guard, which intersects the head set with the open backlog the same way.
/// - A free-text prose/directive/question head (no `#id`) is drainable. Plain
///   operator prose is preserved as work even when it has no imperative verb.
fn head_is_drainable(
    text: &str,
    open_backlog_ids: Option<&std::collections::HashSet<String>>,
    deferred_ids: &std::collections::HashSet<String>,
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
    let components = agent_doc_element::element::parse(content).ok()?;
    let mut found_backlog = false;
    let mut ids = std::collections::HashSet::new();
    for comp in &components {
        if !agent_doc_element::element::is_backlog_component(&comp.name) {
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
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return 0;
    };
    components
        .iter()
        .find(|comp| agent_doc_element::element::is_review_component(&comp.name))
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
    let head = drainable_head_prompt_for_scope(file, content, DrainScope::Supervisor)?;
    let stripped = crate::queue::strip_in_progress_marker(&head.text);
    Some(extract_head_id(&stripped).unwrap_or(stripped))
}

fn drainable_head_prompt_for_scope(
    file: &Path,
    content: &str,
    scope: DrainScope,
) -> Option<crate::queue::QueuePrompt> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let (fm, _) = crate::frontmatter::parse_for_file_with_context(content, file, &rc).ok()?;
    if fm.queue_active != Some(true) {
        return None;
    }
    let components = agent_doc_element::element::parse(content).ok()?;
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
    let deferred_ids = match scope {
        DrainScope::InSessionLoop => deferred_backlog_ids(content),
        DrainScope::Supervisor => supervisor_deferred_backlog_ids(content),
    };
    let preset_supplies_directive = queue_component.attrs.contains_key("preset");
    first_drainable_head(
        &activation.entries_after,
        open_backlog.as_ref(),
        &deferred_ids,
        preset_supplies_directive,
    )
    .cloned()
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
    let Ok(components) = agent_doc_element::element::parse(content) else {
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
    let preset_supplies_directive = queue_component.attrs.contains_key("preset");
    activation
        .entries_after
        .iter()
        .filter(|entry| match entry {
            crate::queue::QueueEntry::Prompt(prompt) => head_is_drainable(
                &prompt.text,
                open_backlog.as_ref(),
                &deferred_ids,
                preset_supplies_directive,
            ),
            _ => false,
        })
        .count()
}

/// Recognized actionable directive verbs for go-mode drainability classification
/// (`#goqstall2`). Word-matched anywhere in a normalized head. This is now an
/// affirmative signal only: ordinary prose queue heads stay drainable even when
/// they do not contain one of these verbs, because a natural-language bug report
/// is still operator-authored work.
///
/// `#cleardrainsignal`: deliberately EXCLUDES words that are common nouns in
/// agent-doc's own bug reports (e.g. "document" — "this document has…", "the
/// document model" — appears in nearly every report). Keep this list to verbs
/// that read as imperatives, not nouns, in queue prose; non-artifact prose is
/// preserved separately by the default drainable branch.
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

/// Recurring **imperative command** verbs (`#qimpstrike`). These are executable
/// directives that are valid *every* time they are queued — running `deploy`
/// today does not retire a standing `deploy` directive queued tomorrow. They are
/// therefore NOT one-time answerable tasks and must never be retired by the
/// `#qftbklgstrike` lexical done/backlog matcher or the `#qheadresidue` residue
/// guard (a single common verb like `deploy` is lexically close to many prior
/// `commit + push + deploy` done items, and a response that echoes the head as a
/// `> **Queue prompt:**` quote does not "answer" a standing command).
///
/// This is the single source of truth for the recurring-imperative subclass; both
/// strike sites (`memory_cmd::semantic_queue_strike_matches` and
/// `session_check::queue_head_provenance_guards`) call
/// [`is_recurring_imperative_head`] rather than carrying their own verb list.
///
/// Distinct from [`QUEUE_DIRECTIVE_VERBS`] (the broad go-mode *drainability*
/// signal, which deliberately includes `add`/`fix`/`update`/… — verbs that
/// commonly *lead a one-time task* like "fix the lender email parity"). This
/// narrower set is only the recurring deploy/release-cycle command verbs, so a
/// multi-word prose head that merely *contains* one (`fix the deploy script`) is
/// still a one-time task and can still be legitimately struck.
const RECURRING_IMPERATIVE_COMMAND_VERBS: &[&str] = &[
    "deploy", "commit", "push", "build", "install", "release", "test", "sync", "recycle",
    "publish", "tag", "bump",
];

/// True when a queue head is a **recurring imperative command** (`#qimpstrike`):
/// its normalized text is dominated by a known recurring-imperative command verb
/// (see [`RECURRING_IMPERATIVE_COMMAND_VERBS`]) or is a recurring-command preset
/// token (`#spec-test-commit-push`, `#commit-push`, …) whose id is built entirely
/// from those verbs.
///
/// "Dominated by" = a short head (≤ 3 actionable words) whose *first* word is a
/// recurring-imperative verb, e.g. `deploy`, `commit + push`,
/// `push origin main`. A longer prose head that merely contains such a verb
/// (`fix the deploy script so it retries`) is a one-time task, NOT a recurring
/// command, so it returns false and stays eligible for the legitimate
/// `#qftbklgstrike` restatement strike.
pub(crate) fn is_recurring_imperative_head(text: &str) -> bool {
    let normalized = normalize_queue_head_text(text);
    if normalized.is_empty() {
        return false;
    }
    // A recurring-command *preset token* (`#spec-test-commit-push`, `#commit-push`)
    // is an executable directive: treat it as recurring when its id segments are
    // all recurring-imperative verbs (ignore generic glue like `spec`).
    if let Some(id) = extract_head_id(&normalized) {
        let segments: Vec<&str> = id.split(['-', '_']).filter(|s| !s.is_empty()).collect();
        let verb_segments = segments
            .iter()
            .filter(|s| {
                RECURRING_IMPERATIVE_COMMAND_VERBS.contains(&s.to_ascii_lowercase().as_str())
            })
            .count();
        // A preset built from at least two recurring verbs (`commit-push`,
        // `spec-test-commit-push`) is a recurring-command preset.
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
    // Dominated-by: the head LEADS with a recurring-imperative verb. `deploy`,
    // `commit + push`, `push origin main` qualify; `the deploy failed` does not
    // (leads with `the`).
    RECURRING_IMPERATIVE_COMMAND_VERBS.contains(&words[0].as_str())
}

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

/// True when a queue head, after stripping the leading bullet and `:shortcode:`
/// pins, begins with a markdown bold span (`**…**`) — the shape of an agent
/// response-fragment summary bullet (`**migrate** — folded …`, `**Resolved 3
/// reviews** → archived …`) that a cross-doc CRDT merge can splice into this
/// queue (#qcontam). Genuine operator directives lead with a verb / `#id` / plain
/// text, never a bold summary span, so this is a safe noise signal. Unlike
/// [`normalize_queue_head_text`] this keeps `*` so the bold span survives.
fn leads_with_markdown_bold_report(text: &str) -> bool {
    let mut s = text.trim();
    if let Some(rest) = s.strip_prefix('-') {
        s = rest.trim_start();
    }
    // Strip leading `:shortcode:` pins (e.g. `:pushpin:`), repeatedly.
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

/// True for one-line console/status artifacts that are safe to classify as
/// non-work queue noise. Plain prose reports deliberately do not match here.
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

/// Whether a queue `Prompt` head is auto-drainable in go-mode (`#goqstall2`).
///
/// A pre-materialized `## Queue` block can carry free-text lines that are not
/// actionable drain targets — pasted console evidence or agent response fragments.
/// Those churn no-op closeouts because the continuation walk treats every
/// `Prompt` as a ready head.
///
/// A head is drainable iff it carries a `#id` / `[#id]` (the
/// `[clean-session]`/`[operator-verify]` defer is applied separately by id), ends
/// with a question mark, contains a recognized imperative directive verb, or lives
/// in a preset-bearing queue where the preset supplies the directive verb. Plain
/// operator prose is also drainable by default; only structural/log artifacts are
/// inert **noise** surfaced as `queue_stale_noise_lines`.
pub(crate) fn is_drainable_queue_head(text: &str) -> bool {
    is_drainable_queue_head_with_context(text, false)
}

/// True when `text` is a non-drainable **noise** queue head: the inverse of
/// [`is_drainable_queue_head_with_context`]. Pasted console output and
/// agent-response fragments can never drain and only churn the go-mode loop.
/// Plain operator prose is not noise. Centralized so `queue prune-noise` strikes
/// exactly the entries [`queue_stale_noise_lines`] counts (`#goqstall2`).
/// `preset_supplies_directive` must match the active queue's `preset` attribute
/// so a preset-bearing queue classifies predicate-proven noise identically to the
/// counter.
pub(crate) fn is_noise_queue_head(text: &str, preset_supplies_directive: bool) -> bool {
    !is_drainable_queue_head_with_context(text, preset_supplies_directive)
}

fn is_drainable_queue_head_with_context(text: &str, preset_supplies_directive: bool) -> bool {
    // Pasted console-output / agent-response-fragment evidence is NOISE, not a
    // drain target when it has no operator prose lead. A prose bug report followed
    // by fenced diagnostics is still drainable: the prose lead is the object to
    // act on, even without a queue-level preset.
    //
    // The non-prose markers below still demote the head even under a preset:
    //   1. an agent component or boundary comment (`<!-- agent:` / `agent:boundary`)
    //      — a spliced agent response artifact;
    //   2. a leading markdown bold summary span (`**…**`) — an agent response bullet
    //      (e.g. a cross-doc `**migrate** — folded … agent:review` fragment that a
    //      CRDT merge split out of its fence into this queue). (#qcontam)
    // Checked BEFORE the `#id` fast-path so a fragment carrying a stray cross-doc
    // id is still demoted.
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
    // A harness slash command (`/clear`, `/model sonnet`, `/code-review`) is a
    // drainable command head, submitted to the owner pane — not prose noise.
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
    // Default toward preserving operator-authored work. A prose bug report like
    // "Queue items are being struck without being worked on" is actionable even
    // though it is not phrased as an imperative.
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

/// Count of active queue `Prompt` heads that are non-drainable **noise**
/// (`#goqstall2`): structural/log artifacts that are not `#id` heads, slash
/// commands, questions, directives, or ordinary prose prompts. Used by
/// `session-check` to surface a `queue_stale_noise_lines=N` diagnostic so the
/// operator can clear pasted console evidence that would otherwise churn the
/// go-mode drain. The field name is retained for compatibility; the predicate is
/// exact noise, not a license to delete fresh operator queue edits.
pub fn queue_stale_noise_lines(file: &Path) -> usize {
    let Ok(content) = std::fs::read_to_string(file) else {
        return 0;
    };
    let Ok(components) = agent_doc_element::element::parse(&content) else {
        return 0;
    };
    let Some(queue_component) = components.iter().find(|c| c.name == "queue") else {
        return 0;
    };
    let body = &content[queue_component.open_end..queue_component.close_start];
    let Ok(entries) = crate::queue::parse(body) else {
        return 0;
    };
    let preset_supplies_directive = queue_component.attrs.contains_key("preset");
    entries
        .iter()
        .filter(|entry| match entry {
            // Counts must match EXACTLY what `queue prune-noise` excises
            // (#qnoise-multiline-strike): a Prompt is noise when not drainable (the
            // classifier demotes multi-line / fenced text), and a pasted-evidence
            // `Freeform` line (bare ``` console fence / prose head) is noise while
            // `---`/`~~~` separators and `re [#id]` references are not.
            crate::queue::QueueEntry::Prompt(prompt) => {
                !is_drainable_queue_head_with_context(&prompt.text, preset_supplies_directive)
            }
            crate::queue::QueueEntry::Freeform(line) => crate::queue::is_noise_freeform_line(line),
            _ => false,
        })
        .count()
}

/// Extract the backlog `#id` from a queue prompt like `do [#id] ...` or `#id ...`.
pub(crate) fn extract_head_id(prompt: &str) -> Option<String> {
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

    /// `#degraded-ipc-no-stall`: the shared no-stall guidance must distinguish
    /// proven degraded editor transport from unproven IPC/direct-write fallback
    /// so neither preflight nor session-check can drift into licensing data loss.
    #[test]
    fn continuation_guidance_names_degraded_ipc_and_exhaustive_stop_list() {
        let g = CONTINUATION_NO_STALL_GUIDANCE;
        assert!(g.contains("file-IPC"), "must name the file-IPC fallback");
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

    fn write_doc(dir: &Path, prompts: &[&str], queue_active: bool, has_auto: bool) -> PathBuf {
        let queue_attrs = if has_auto { " auto" } else { "" };
        write_doc_with_queue_attrs(dir, prompts, queue_active, queue_attrs)
    }

    /// `#qpausego`: set the document-scope controller queue-control state for a doc.
    fn set_document_queue_control(root: &Path, doc: &Path, state: &str) {
        let conn = agent_doc_sqlite::state_store::open_state_db(root).unwrap();
        let scope_id = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_sqlite::state_store::upsert_queue_control_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueControlInsert {
                scope_kind: "document",
                scope_id: &scope_id,
                state,
                reason: Some("test pause"),
                operation_receipt_id: None,
            },
        )
        .unwrap();
    }

    /// `#qpausego`: with no controller state DB at all, a doc is never reported
    /// as controller-paused (a missing control plane must not wedge a queue).
    #[test]
    fn document_queue_controller_paused_false_without_state_db() {
        let dir = tempfile::tempdir().unwrap();
        // `.agent-doc` must exist for project-root resolution, but no state DB.
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = write_doc(dir.path(), &["do [#a]"], true, true);
        assert!(
            !document_queue_controller_paused(&doc),
            "no state DB means nothing can be controller-paused"
        );
    }

    /// `#qpausego`: an accepted `admin queue pause` (document-scope `paused`
    /// control row) is reported as paused; `resume` clears it.
    #[test]
    fn document_queue_controller_paused_reflects_paused_then_resumed() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#a]"], true, true);

        set_document_queue_control(dir.path(), &doc, "paused");
        assert!(
            document_queue_controller_paused(&doc),
            "an accepted pause must report the queue as controller-paused"
        );

        set_document_queue_control(dir.path(), &doc, "resumed");
        assert!(
            !document_queue_controller_paused(&doc),
            "resume must clear the controller pause"
        );
    }

    /// `#qpausego`: a `paused` controller state must NOT short-circuit
    /// continuation — the attended in-session `/loop` (and `session-check` /
    /// codex-stop, which consult `detect`) keeps draining real queue work. The
    /// pause only suppresses the unattended supervisor idle-watch injection.
    #[test]
    fn detect_still_continues_when_controller_paused() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do something"], true, true);
        assert!(
            detect(&doc).unwrap().is_some(),
            "active auto queue with a live head should require continuation"
        );

        set_document_queue_control(dir.path(), &doc, "paused");
        assert!(
            detect(&doc).unwrap().is_some(),
            "a controller pause must NOT stall the in-session loop continuation"
        );
    }

    fn write_doc_with_queue_attrs(
        dir: &Path,
        prompts: &[&str],
        queue_active: bool,
        queue_attrs: &str,
    ) -> PathBuf {
        let queue: String = prompts.iter().map(|p| format!("- {p}\n")).collect();
        write_doc_with_queue_body(dir, &queue, queue_active, queue_attrs)
    }

    fn write_doc_with_queue_body(
        dir: &Path,
        queue_body: &str,
        queue_active: bool,
        queue_attrs: &str,
    ) -> PathBuf {
        std::fs::create_dir_all(dir.join(".agent-doc/snapshots")).unwrap();
        let doc = dir.join("task.md");
        let content = format!(
            "---\nsession: sid\nagent_doc_format: template\nqueue_active: {queue_active}\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue{queue_attrs} -->\n{queue_body}<!-- /agent:queue -->\n"
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
        assert!(
            !deferred.contains("a"),
            "clean-session drains in-loop (#qcontdrain)"
        );
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
        assert!(
            !deferred.contains("e"),
            "every clean-session head drains in-loop"
        );
        assert!(
            deferred.contains("b"),
            "operator-verify is never drainable by any agent scope"
        );
    }

    #[test]
    fn supervisor_drains_focused_cycle_but_loop_defers_it() {
        // #qfocsup: a [focused-cycle] head is deferred by the in-session loop (it
        // cannot give the fresh context the tag demands) but DRAINED by the
        // supervisor (force-/clear + re-dispatch), so it never strands the queue
        // idle. [operator-verify] stays deferred in BOTH scopes.
        let content = doc_with_backlog(
            &["do [#f]", "do [#o]"],
            &[
                "- [ ] [#f] [focused-cycle] merge-core work",
                "- [ ] [#o] [operator-verify] live drive",
            ],
        );
        let loop_deferred = deferred_backlog_ids(&content);
        assert!(
            loop_deferred.contains("f"),
            "focused-cycle deferred by in-session loop"
        );
        assert!(
            loop_deferred.contains("o"),
            "operator-verify deferred by in-session loop"
        );

        let sup_deferred = supervisor_deferred_backlog_ids(&content);
        assert!(
            !sup_deferred.contains("f"),
            "focused-cycle is supervisor-drainable (#qfocsup)"
        );
        assert!(
            sup_deferred.contains("o"),
            "operator-verify never drainable by any scope"
        );
    }

    #[test]
    fn focused_cycle_head_drains_for_supervisor_not_in_session_loop() {
        // The queue is NOT idle when only a [focused-cycle] head remains: the
        // supervisor picks it up (and force-/clears first), while the in-session
        // loop yields it (drainable_head_count == 0).
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("d.md");
        let content = doc_with_backlog(
            &["do [#f]"],
            &["- [ ] [#f] [focused-cycle] merge-core work"],
        );
        std::fs::write(&doc, &content).unwrap();

        assert_eq!(
            live_drainable_continuation_head(&doc, &content).as_deref(),
            Some("f"),
            "supervisor drains the focused-cycle head (clear-and-continue)"
        );
        assert_eq!(
            drainable_head_count(&doc, &content),
            0,
            "in-session loop yields the focused-cycle head to the supervisor"
        );
        assert!(
            head_requires_context_reset_in(&content, "do [#f]"),
            "supervisor force-/clears before a focused-cycle head"
        );
    }

    #[test]
    fn head_requires_context_reset_covers_clean_session_and_focused_cycle() {
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
        assert!(head_requires_context_reset_in(&content, "f"));
        assert!(
            !head_requires_context_reset_in(&content, "o"),
            "operator-verify is not a context-reset (it is never auto-dispatched)"
        );
        assert!(!head_requires_context_reset_in(&content, "p"));
        // clean-session-only check stays narrow (focused-cycle excluded).
        assert!(head_requires_clean_session_in(&content, "c"));
        assert!(!head_requires_clean_session_in(&content, "f"));
        assert!(!head_requires_focused_cycle_in(&content, "c"));
        assert!(head_requires_focused_cycle_in(&content, "f"));
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
        // #cleardrainsignal: a deferred head is excluded; prose reports and
        // directive heads both count as drainable work.
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
        // #b deferred (operator-verify), the prose report and #c remain real work.
        assert_eq!(drainable_head_count(&doc, &content), 2);
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
        // #cleardrainsignal: a pure pasted console block (fenced ```) is noise
        // even when it incidentally contains directive words.
        let fenced = "```\n[route] target tmux session: 0\nError: run blocked\n```";
        assert!(!is_drainable_queue_head(fenced));
        // A prose bug report with diagnostic evidence is still operator-authored
        // queue work and must not be demoted to noise.
        let report = ":pushpin: JB `Run Agent Doc` should self-heal.\n```\n[route] target tmux session: 0\nError: dispatch blocked\n```";
        assert!(is_drainable_queue_head(report));
        // A genuine inline directive (no fence) stays drainable.
        assert!(is_drainable_queue_head("run the full test suite"));
    }

    #[test]
    fn preset_queue_treats_prompt_lines_as_drainable_directives() {
        // #goqnoise: with a queue-level preset, the preset supplies the verb for
        // each non-empty prompt line; the line itself is the object to act on.
        let dir = tempfile::tempdir().unwrap();
        let cpa_line = "\"CPA / attorney letter or supporting document\" in https://example.test/accreditation and the dialog should allow multiple files. Use our standard multi-file upload component.";
        let doc = write_doc_with_queue_attrs(
            dir.path(),
            &[cpa_line, "deploy"],
            true,
            " auto preset=\"#spec-test-commit-push\" go",
        );
        let content = std::fs::read_to_string(&doc).unwrap();

        let continuation = detect(&doc).unwrap().expect("preset queue should drain");
        assert_eq!(continuation.head_prompt, cpa_line);
        assert_eq!(
            live_drainable_continuation_head(&doc, &content).as_deref(),
            Some(cpa_line)
        );
        assert_eq!(drainable_head_count(&doc, &content), 2);
        assert_eq!(queue_stale_noise_lines(&doc), 0);
    }

    #[test]
    fn is_drainable_queue_head_treats_cross_doc_response_fragment_as_noise() {
        // #qcontam: a cross-doc agent response-fragment bullet that a CRDT merge
        // split into this queue is noise even under a preset, even though it has no
        // fence and incidentally contains directive verbs / stray ids.
        let bold_lead = ":pushpin: **migrate** — folded the 8 legacy gated `agent:backlog` items into `agent:review` (clears `legacy_gated_in_backlog`).";
        assert!(!is_drainable_queue_head(bold_lead));
        assert!(!is_drainable_queue_head_with_context(bold_lead, true));
        let resolved_lead = ":pushpin: **Resolved 3 genuinely-finished reviews** → archived to `agent:done`: `#2qrx` (apply done).";
        assert!(!is_drainable_queue_head_with_context(resolved_lead, true));
        // An agent component/boundary comment spliced into a head is also noise.
        let boundary = ":pushpin: stray response tail\n<!-- agent:boundary:7c96f9ce -->";
        assert!(!is_drainable_queue_head_with_context(boundary, true));
        // A genuine operator directive (no bold lead, no agent markers) stays
        // drainable under a preset, even when long.
        let legit = "All upload preview dialogs should be full screen. deploy";
        assert!(is_drainable_queue_head_with_context(legit, true));
        // A plain `**bold**`-free imperative is still drainable without a preset.
        assert!(is_drainable_queue_head("run the full test suite"));
    }

    #[test]
    fn preset_queue_treats_prose_plus_fenced_diagnostics_as_drainable() {
        // A queue-level preset supplies the directive; a prose bug report followed
        // by fenced diagnostics is real work, not stale noise.
        let dir = tempfile::tempdir().unwrap();
        let fenced = ":pushpin: JB `Run Agent Doc` on agent-loop.md after switching from claude to codex. The actor record did not switch.\n```\n[route] target tmux session: 0\nError: authoritative actor record is bound to harness claude-code, not codex\n```";
        let queue_body = format!("~~~prompt\n{fenced}\n~~~\n");
        let doc = write_doc_with_queue_body(
            dir.path(),
            &queue_body,
            true,
            " auto preset=\"#spec-test-commit-push\" go",
        );
        let content = std::fs::read_to_string(&doc).unwrap();

        assert_eq!(
            detect(&doc)
                .unwrap()
                .map(|continuation| continuation.head_prompt),
            Some(fenced.to_string())
        );
        assert_eq!(
            live_drainable_continuation_head(&doc, &content).as_deref(),
            Some(fenced)
        );
        assert_eq!(drainable_head_count(&doc, &content), 1);
        assert_eq!(queue_stale_noise_lines(&doc), 0);
    }

    #[test]
    fn preset_separator_freetext_drains_ahead_of_operator_verify_pins() {
        // Live regression: a free-text bug report typed before a `---` separator
        // must not be skipped as Freeform/noise, or preflight sees only the
        // operator-verify mirror heads and stalls with 0 drainable work.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("task.md");
        let prompt = concat!(
            "JB `Run Agent Doc` on agent-loop.md after switching from claude to codex. ",
            "The session switched from claude to codex, but the actor record did not switch.\n",
            "```\n",
            "[route] target tmux session: 0\n",
            "Error: authoritative actor record is bound to harness claude-code, not codex\n",
            "```",
        );
        let content = format!(
            "---\nsession: sid\nagent_doc_format: template\nqueue_active: true\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue priority preset=\"#spec-test-build-install-commit-push\" priority go -->\n{prompt}\n---\n- :pushpin: do [#ov]\n<!-- /agent:queue -->\n\n\
## Backlog\n\n<!-- agent:backlog priority queue -->\n- [ ] [#ov] [operator-verify] live drive\n<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();

        assert_eq!(
            detect(&doc)
                .unwrap()
                .map(|continuation| continuation.head_prompt),
            Some(prompt.to_string())
        );
        assert_eq!(
            live_drainable_continuation_head(&doc, &content).as_deref(),
            Some(prompt)
        );
        assert_eq!(drainable_head_count(&doc, &content), 1);
        assert_eq!(queue_stale_noise_lines(&doc), 0);
    }

    #[test]
    fn preset_queue_treats_all_log_fenced_paste_as_noise() {
        // #goqnoise: preset context must not re-admit pure console evidence.
        let dir = tempfile::tempdir().unwrap();
        let fenced = "```\n[route] target tmux session: 0\nError: dispatch blocked\n```";
        let queue_body = format!("~~~prompt\n{fenced}\n~~~\n");
        let doc = write_doc_with_queue_body(
            dir.path(),
            &queue_body,
            true,
            " auto preset=\"#spec-test-commit-push\" go",
        );
        let content = std::fs::read_to_string(&doc).unwrap();

        assert!(detect(&doc).unwrap().is_none());
        assert!(live_drainable_continuation_head(&doc, &content).is_none());
        assert_eq!(drainable_head_count(&doc, &content), 0);
        assert_eq!(queue_stale_noise_lines(&doc), 1);
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
    fn detect_yields_when_supervisor_requests_recycle_yield() {
        // `#wd40` / `#staleloop-recycle-restart`: a stale supervisor that can never
        // reach its own recycle boundary during a continuously self-draining session
        // writes a recycle-yield request; while it is live the in-session loop must
        // see NO continuation (so it ends its turn and the execve recycle fires),
        // even though the active queue still has drainable heads. Clearing the
        // request restores normal continuation so the drain resumes post-recycle.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &["do [#seopdp] next", "do [#third]"],
            true,
            true,
        );
        let doc_str = doc.to_string_lossy().to_string();

        // Baseline: continuation is owed.
        assert!(detect(&doc).unwrap().is_some());

        // A live recycle-yield request suppresses continuation entirely.
        crate::recycle_yield::request_recycle_yield(
            &doc_str,
            crate::recycle_yield::RECYCLE_YIELD_STALE_BINARY,
        )
        .unwrap();
        assert!(
            detect(&doc).unwrap().is_none(),
            "a pending recycle-yield must make the in-session loop yield"
        );

        // Clearing the request hands the drain back so the loop resumes.
        crate::recycle_yield::clear_recycle_yield(&doc_str);
        assert!(
            detect(&doc).unwrap().is_some(),
            "clearing the recycle-yield must restore normal continuation"
        );
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
        // #goqstall2: `#id` heads, directives, questions, and prose reports are
        // drainable.
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
        assert!(is_drainable_queue_head("deploy"));
        assert!(is_drainable_queue_head(
            "- I'm still seeing JB `File Cache Conflict` dialogs. There should be 0."
        ));
        assert!(is_drainable_queue_head(
            ":pushpin: JB `Compact Exchange` has a partially uncommitted response."
        ));
        assert!(is_drainable_queue_head(
            "Queue items are being struck without being worked on."
        ));
        assert!(is_drainable_queue_head(
            "- This document has `agent:backlog priority queue`...but the backlog items are not being added to the `agent:queue`."
        ));
        assert!(is_drainable_queue_head(
            ":pushpin: Ensure the markdown AST handles strings and code blocks. Tags within the blocks should be treated as content, not as part of the document tags."
        ));

        // Empty or artifact-only heads remain noise.
        assert!(!is_drainable_queue_head("- "));
        assert!(!is_drainable_queue_head(":pushpin:"));
        assert!(!is_drainable_queue_head("[route] target tmux session: 0"));
        assert!(!is_drainable_queue_head(
            "[error] dispatch blocked: only the gated #5eq8 remains."
        ));
        assert!(is_drainable_queue_head(
            "Error: queue items are being struck without being worked on."
        ));
    }

    #[test]
    fn recurring_imperative_head_classification() {
        // #qimpstrike: recurring imperative command heads are executable
        // directives, exempt from both strike sites.
        for head in [
            "deploy",
            "- deploy",
            ":pushpin: deploy",
            "commit",
            "push",
            "build",
            "install",
            "release",
            "test",
            "sync",
            "recycle",
            "publish",
            "commit + push",
            "push origin main",
            "#commit-push",
            "#spec-test-commit-push",
        ] {
            assert!(
                is_recurring_imperative_head(head),
                "{head:?} should classify as a recurring-imperative head"
            );
            // And it stays drainable (dispatchable through convergence), never
            // demoted to noise.
            assert!(
                is_drainable_queue_head(head),
                "{head:?} must remain drainable"
            );
        }

        // NOT recurring-imperative: multi-word prose tasks that merely mention a
        // command verb, prose bug reports, questions, unrelated verbs. These stay
        // eligible for the legitimate `#qftbklgstrike` restatement strike.
        for head in [
            "fix the deploy script so it retries on a transient 500",
            "the deploy failed last night",
            "investigate why the build is slow",
            "add a dark mode toggle to the settings panel",
            "why is the commit history squashed?",
            "Queue items are being struck without being worked on.",
            "#lender-card-msg-email",
        ] {
            assert!(
                !is_recurring_imperative_head(head),
                "{head:?} must NOT classify as a recurring-imperative head"
            );
        }
    }

    #[test]
    fn detect_serves_prose_report_before_later_directive() {
        // #freshprosequeue: a natural-language bug report is work even without an
        // imperative verb, so it stays the continuation head instead of being
        // skipped/struck as noise.
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
        assert_eq!(
            continuation.head_prompt,
            "I'm still seeing JB File Cache Conflict dialogs. There should be 0."
        );
        assert_eq!(queue_stale_noise_lines(&doc), 0);
    }

    #[test]
    fn detect_quiesces_when_only_artifact_noise_heads_remain() {
        // #goqstall2: a queue of only console/status artifacts is NOT a stall —
        // continuation is not required and the lines are counted as predicate-
        // proven noise.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &["[route] target tmux session: 0", "[error] dispatch blocked"],
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
    fn live_drainable_head_skips_artifact_noise_and_lands_on_directive() {
        // #qchurn: the supervisor idle-watch dispatch head must skip inert
        // artifact lines and land on real work — matching detect(), so the watch
        // does not churn no-op /agent-doc cycles when the only ready head is noise.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &[
                "[route] target tmux session: 0",
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
    fn live_drainable_head_none_when_only_artifact_noise() {
        // #qchurn: a queue of only artifact lines yields NO drainable idle-watch
        // head, so the supervisor stops re-dispatching. The unfiltered
        // live_continuation_head still returns the first head, proving the filter
        // is what quiesces the churn.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &["[route] target tmux session: 0", "[error] dispatch blocked"],
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
