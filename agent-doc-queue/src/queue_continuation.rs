//! Pure queue-continuation drainability policy.
//!
//! This module owns content-only decisions for active queue continuation,
//! drainable heads, deferred backlog ids, recurring imperative heads, and queue
//! noise classification. Callers own file IO, snapshots, controller state,
//! sidecars, and marker persistence.

use std::collections::HashSet;

use agent_doc_element::element;
use agent_doc_element_backlog::backlog;
use agent_doc_frontmatter::frontmatter;

use crate::document_queue::{self, QueueEntry, QueuePrompt};

/// Shared non-stall guidance surfaced wherever `queue_continuation_required ==
/// true`. Centralizing the wording keeps preflight JSON
/// (`queue_continuation_guidance`) and `session-check` stdout in agreement
/// (`#degraded-ipc-no-stall`).
///
/// The failure this guards against: a `finalize` that reached `committed` +
/// `session-check ok` through a **proven file-IPC editor fallback** (socket ack
/// timeouts / a stale or wedged route-owned supervisor) is a *successful*
/// closeout; the in-session loop does not depend on the socket itself. The
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

/// Drain scope for computing which backlog ids are deferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainScope {
    /// In-session loop: defers `[operator-verify]` and `[focused-cycle]`.
    InSessionLoop,
    /// Supervisor clear-and-continue: defers `[operator-verify]` only.
    Supervisor,
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
    let (queue_facts, activation) = active_queue(content)?;
    let open_backlog = open_backlog_ids_from_content(content);
    let deferred_ids = match scope {
        DrainScope::InSessionLoop => deferred_backlog_ids(content),
        DrainScope::Supervisor => supervisor_deferred_backlog_ids(content),
    };
    first_drainable_head(
        &activation.entries_after,
        open_backlog.as_ref(),
        &deferred_ids,
        queue_facts.preset_supplies_directive,
    )
    .cloned()
}

/// Live drainable active queue head for `scope`.
pub fn live_drainable_continuation_head(content: &str, scope: DrainScope) -> Option<String> {
    let head = drainable_head_prompt_for_scope(content, scope)?;
    let stripped = document_queue::strip_in_progress_marker(&head.text);
    Some(extract_head_id(&stripped).unwrap_or(stripped))
}

/// Count agent-drainable heads in the active queue for the in-session loop.
pub fn drainable_head_count(content: &str) -> usize {
    let Some((queue_facts, activation)) = active_queue(content) else {
        return 0;
    };
    let open_backlog = open_backlog_ids_from_content(content);
    let deferred_ids = deferred_backlog_ids(content);
    activation
        .entries_after
        .iter()
        .filter(|entry| match entry {
            QueueEntry::Prompt(prompt) => head_is_drainable(
                &prompt.text,
                open_backlog.as_ref(),
                &deferred_ids,
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
            preset_supplies_directive: queue_component.attrs.contains_key("preset"),
        },
        entries,
    ))
}

fn active_queue(content: &str) -> Option<(QueueFacts, document_queue::QueueActivation)> {
    let (fm, _) = frontmatter::parse(content).ok()?;
    if fm.queue_active != Some(true) {
        return None;
    }
    let (queue_facts, entries) = queue_component_entries(content)?;
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
    deferred_ids: &HashSet<String>,
    preset_supplies_directive: bool,
) -> Option<&'a QueuePrompt> {
    entries_after.iter().find_map(|entry| match entry {
        QueueEntry::Prompt(prompt) => {
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

fn head_is_drainable(
    text: &str,
    open_backlog_ids: Option<&HashSet<String>>,
    deferred_ids: &HashSet<String>,
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
                Some(open) => open.contains(&norm),
                None => true,
            }
        }
        None => true,
    }
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
## Queue\n\n<!-- agent:queue auto -->\n{queue}<!-- /agent:queue -->\n"
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
