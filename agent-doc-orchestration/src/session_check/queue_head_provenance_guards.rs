use super::*;

pub(crate) fn check_expect_done_or_gate_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): resolve guard mode from the cached frontmatter slot.
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }

    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.expect_done_or_gate_ids.is_empty() {
        return Ok(GuardResult::None);
    }
    // Only enforce once the cycle has closed with a committed response. An open
    // cycle is still mid-flight; a no-response commit never sets `capture_id`.
    if state.is_open() {
        return Ok(GuardResult::None);
    }
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if capture.state != crate::capture::CaptureState::Committed {
        return Ok(GuardResult::None);
    }
    if capture
        .response_body
        .contains("<!-- no-pending-done-guard -->")
    {
        return Ok(GuardResult::None);
    }

    let resolved: std::collections::HashSet<String> = state
        .pending_done_ids
        .iter()
        .chain(state.pending_kept_open_ids.iter())
        .chain(state.reaped_pending_ids.iter())
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();

    let open_backlog: std::collections::HashSet<String> =
        open_backlog_ids(file)?.into_iter().collect();

    let mut unresolved: Vec<String> = Vec::new();
    for id in state
        .expect_done_or_gate_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
    {
        if resolved.contains(&id) {
            continue;
        }
        if !open_backlog.contains(&id) {
            continue;
        }
        if !unresolved.iter().any(|existing| existing == &id) {
            unresolved.push(id);
        }
    }

    if unresolved.is_empty() {
        return Ok(GuardResult::None);
    }

    let ids = unresolved
        .iter()
        .map(|id| format!("#{}", id))
        .collect::<Vec<_>>()
        .join(", ");
    let done_hint = unresolved
        .iter()
        .map(|id| format!("--done {}", id))
        .collect::<Vec<_>>()
        .join(" ");
    let repair = format!(
        "agent-doc write {} {} --pending-only --commit",
        file.display(),
        done_hint
    );
    let warn_line = format!(
        "[session-check] warn: `do #id` directive resolved this cycle but tracked target {} is still open in agent:backlog with no `--done`, `--pending-gate`, or kept-open edit recorded",
        ids
    );

    crate::ops_log::log_op(
        file,
        &format!(
            "expect_done_or_gate_guard_fired file={} unresolved={}",
            file.display(),
            unresolved.join(",")
        ),
    );

    Ok(match mode {
        crate::frontmatter::PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!(
                "[session-check] hint: repair with `{}`, run `--pending-gate <id>` if review/external validation remains, or add `pending_done_guard: off` when the item should stay open",
                repair
            ),
        ]),
        crate::frontmatter::PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: repair with `{}`, run `--pending-gate <id>` if review/external validation remains, or set pending_done_guard = \"warn\" to downgrade",
            warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
            repair
        )),
        crate::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

/// `do [#id]` target ids present in a committed document's `agent:queue`
/// component. Used by `#queue-clear-unrun-items` to decide which recorded
/// preflight heads are still queued (preserved) vs removed this cycle.
pub(crate) fn committed_queue_head_ids(content: &str) -> Vec<String> {
    let Ok(components) = crate::component::parse(content) else {
        return Vec::new();
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return Vec::new();
    };
    do_directive_target_ids(&[queue.content(content).to_string()])
}

/// `do [#id]` target ids for the current live queue head only.
pub(crate) fn committed_current_queue_head_ids(content: &str) -> Vec<String> {
    let Ok(components) = crate::component::parse(content) else {
        return Vec::new();
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return Vec::new();
    };
    let entries = crate::queue::parse(queue.content(content)).unwrap_or_default();
    let Some(head) = crate::queue::first_prompt(&entries) else {
        return Vec::new();
    };
    do_directive_target_ids(std::slice::from_ref(&head.text))
}

/// `#queue-clear-unrun-items`: an active `agent:queue` head is executable user
/// intent. A closeout / reset / commit may delete a runnable `do [#id]` head
/// only with durable proof that it was consumed (this cycle's directive target,
/// owned by `#do-id-closeout-open-backlog`), resolved (its `#id` left
/// `agent:backlog` via done/gate/reap), or removed by an explicit user edit.
/// When a head present in the visible queue at preflight disappears from the
/// committed queue while its `#id` is STILL OPEN in `agent:backlog` and the
/// cycle never targeted it, fail closed and name each lost id so the queue can
/// be restored. Suppress an intentional user removal with
/// `<!-- no-queue-removal-guard -->`.
pub(crate) fn check_queue_head_removal_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): resolve guard mode from the cached frontmatter slot.
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.active_queue_heads.is_empty() {
        return Ok(GuardResult::None);
    }
    // Only enforce on a committed closeout; an open cycle is still mid-flight and
    // may not have written the final queue yet.
    if state.is_open() {
        return Ok(GuardResult::None);
    }
    let recorded_ids = do_directive_target_ids(&state.active_queue_heads);
    if recorded_ids.is_empty() {
        return Ok(GuardResult::None);
    }
    // Phase 6 (#lr-content-6): cached document content.
    let content = rc.doc_content();
    // Explicit user removal already reconciled — do not second-guess it.
    if content.contains("<!-- no-queue-removal-guard -->") {
        return Ok(GuardResult::None);
    }

    let still_queued: std::collections::HashSet<String> = committed_queue_head_ids(&content)
        .into_iter()
        .map(|id| crate::pending::normalize_pending_id(&id))
        .collect();
    let open_backlog: std::collections::HashSet<String> =
        open_backlog_ids(file)?.into_iter().collect();
    // Lifecycle proof: ids the cycle explicitly resolved (done/reaped/gated) or
    // chose to keep open via an explicit edit. A done/gate/reap also removes the
    // id from `open_backlog`, so this set is a defensive superset.
    let mut resolved: std::collections::HashSet<String> =
        crate::cycle_state::resolved_pending_ids(file)?;
    resolved.extend(
        state
            .pending_gated_ids
            .iter()
            .chain(state.pending_kept_open_ids.iter())
            .map(|id| crate::pending::normalize_pending_id(id)),
    );
    // This cycle's `do [#id]` directive targets are owned by the
    // `expect_done_or_gate` guard, which reports the open-target class with a
    // more specific repair. Skip them here to avoid double-firing.
    let directive_targets: std::collections::HashSet<String> = state
        .expect_done_or_gate_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .collect();

    let mut lost: Vec<String> = Vec::new();
    for id in recorded_ids {
        let norm = crate::pending::normalize_pending_id(&id);
        if norm.is_empty() {
            continue;
        }
        if still_queued.contains(&norm) {
            continue; // head preserved in the committed queue
        }
        if !open_backlog.contains(&norm) {
            continue; // backlog item resolved / removed → deletion proven
        }
        if resolved.contains(&norm) || directive_targets.contains(&norm) {
            continue; // explicit lifecycle proof or sibling-owned target
        }
        if !lost.iter().any(|existing| existing == &norm) {
            lost.push(norm);
        }
    }

    if lost.is_empty() {
        return Ok(GuardResult::None);
    }

    let ids = lost
        .iter()
        .map(|id| format!("#{}", id))
        .collect::<Vec<_>>()
        .join(", ");
    crate::ops_log::log_op(
        file,
        &format!(
            "queue_head_removal_guard_fired file={} lost={}",
            file.display(),
            lost.join(",")
        ),
    );
    let warn_line = format!(
        "[session-check] warn: runnable agent:queue head(s) {} were removed from the committed queue but their backlog item(s) are still open in agent:backlog, and the cycle never consumed, completed, gated, or reaped them — unrun queue work was silently dropped",
        ids
    );
    let repair = format!(
        "restore the dropped head(s) to `agent:queue` (or resolve each id with `--done`/`--pending-gate`), then re-run `agent-doc write --commit {}`; add `<!-- no-queue-removal-guard -->` to the response if the removal was an explicit user edit",
        file.display()
    );

    Ok(match mode {
        crate::frontmatter::PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!("[session-check] hint: {repair} (see #queue-clear-unrun-items)"),
        ]),
        crate::frontmatter::PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: {repair} (see #queue-clear-unrun-items)",
            warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
        )),
        crate::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

/// `#lr-queue-patchback-miss`: require committed-response / deferral
/// proof for each free-text (non-`do [#id]`) queue head recorded at preflight.
/// Free-text heads have no backlog id, so the guard checks that: (a) the head
/// text is still present in the committed queue (deferral / not yet consumed),
/// or (b) a committed `### Re:` response exists that plausibly answers it. A
/// binary consume marker by itself is not proof: the answer must be visible in
/// committed `agent:exchange` history, normally via the queue-prompt echo.
pub(crate) fn check_free_text_queue_head_provenance(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.active_free_text_queue_heads.is_empty() {
        return Ok(GuardResult::None);
    }
    if state.is_open() {
        return Ok(GuardResult::None);
    }
    let content = rc.doc_content();
    if content.contains("<!-- no-free-text-queue-head-guard -->") {
        return Ok(GuardResult::None);
    }
    let Ok(components) = crate::component::parse(&content) else {
        return Ok(GuardResult::None);
    };
    let committed_queue_text: String = components
        .iter()
        .find(|c| c.name == "queue")
        .map(|c| c.content(&content).to_string())
        .unwrap_or_default();
    let mut unresolved: Vec<String> = Vec::new();
    for head in &state.active_free_text_queue_heads {
        let normalized = head.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        if committed_queue_text
            .to_ascii_lowercase()
            .contains(&normalized)
        {
            continue;
        }
        if response_head_plausibly_answers(&content, head) {
            continue;
        }
        unresolved.push(head.clone());
    }
    if unresolved.is_empty() {
        return Ok(GuardResult::None);
    }
    let heads_text = unresolved
        .iter()
        .map(|h| format!("\"{}\"", h))
        .collect::<Vec<_>>()
        .join(", ");
    crate::ops_log::log_op(
        file,
        &format!(
            "free_text_queue_head_provenance_guard_fired file={} unresolved={}",
            file.display(),
            heads_text
        ),
    );
    let warn_line = format!(
        "[session-check] warn: free-text agent:queue head(s) {heads_text} were seen at preflight but have no committed response/echo or explicit deferral proof in the closeout — the prompt may have been silently lost"
    );
    let repair = format!(
        "either respond to the unresolved head(s) and run `agent-doc finalize {}`, or add `<!-- no-free-text-queue-head-guard -->` if the removal was intentional",
        file.display()
    );
    Ok(match mode {
        crate::frontmatter::PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!("[session-check] hint: {repair} (see #lr-queue-patchback-miss)"),
        ]),
        crate::frontmatter::PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: {repair} (see #lr-queue-patchback-miss)",
            warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
        )),
        crate::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

pub(crate) fn response_head_plausibly_answers(content: &str, head: &str) -> bool {
    let head_words: Vec<&str> = head
        .split_whitespace()
        .filter(|w| {
            w.len() > 3
                && !matches!(
                    w.to_ascii_lowercase().as_str(),
                    "the"
                        | "this"
                        | "that"
                        | "with"
                        | "from"
                        | "also"
                        | "does"
                        | "what"
                        | "when"
                        | "how"
                )
        })
        .collect();
    if head_words.is_empty() {
        return false;
    }
    let lower = content.to_ascii_lowercase();
    let mut matched = 0;
    for word in &head_words {
        if lower.contains(&word.to_ascii_lowercase()) {
            matched += 1;
        }
    }
    matched * 2 >= head_words.len()
}

/// Tight list of "deferred live work" phrases that, combined with a shipped
/// signal, indicate a `do [#id]` turn shipped a repo phase but left live
/// deploy / sync / verification / approval work for a later phase
/// (`#do-id-partial-closeout-state`). Kept narrow to avoid false positives on
/// ordinary closeout prose.
pub(crate) const PARTIAL_CLOSEOUT_REMAINING_PHRASES: &[&str] = &[
    "not deployed",
    "not yet deployed",
    "deploy remains",
    "deployment remains",
    "deploy/",
    "live verification",
    "live verify",
    "live-verify",
    "external validation remains",
    "awaiting approval",
    "awaiting user",
    "user approval",
    "sync remains",
    "feed sync",
    "merchant center",
    "live ads",
    "remains: deploy",
];

pub(crate) fn text_has_shipped_signal(lower: &str) -> bool {
    (lower.contains("committed")
        || lower.contains("commit + push")
        || lower.contains("commit and push"))
        && lower.contains("push")
}

pub(crate) fn text_has_partial_remaining_signal(lower: &str) -> bool {
    PARTIAL_CLOSEOUT_REMAINING_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
}

/// Tight list of "blocked / still needs future action" phrases that, combined
/// with a directed id gated this cycle, indicate a `do [#id]` closeout reported
/// the work is still incomplete and needs more agent execution — distinct from
/// a clean implementation-complete review gate. Kept narrow (no bare "blocked"
/// or generic "requires"/"until") so ordinary review/closeout prose does not
/// trip the guard.
pub(crate) const BLOCKED_FUTURE_ACTION_PHRASES: &[&str] = &[
    "remains blocked",
    "still blocked",
    "is blocked",
    "are blocked",
    "blocked on",
    "blocked by",
    "blocked:",
    "blocked until",
    "next step to complete",
    "next steps to complete",
    "steps to complete",
    "cannot complete until",
    "can't complete until",
    "still needs to",
    "still need to",
    "must remove",
    "must delete",
    "must expire",
    "needs to be removed",
    "no live cutover",
    "waiting on approval",
    "awaiting approval",
    "needs approval before",
    "requires approval before",
    "deliberately delete",
    "get approval",
];

/// Explicit "no follow-up needed" justifications that satisfy a blocked-shape
/// closeout for a genuinely review-only gate. Keep aligned with the repair hint.
pub(crate) const NO_FOLLOWUP_JUSTIFICATION_PHRASES: &[&str] = &[
    "no additional backlog follow-up",
    "no additional follow-up is needed",
    "no follow-up backlog",
    "no further backlog",
    "no actionable backlog follow-up",
    "no remaining backlog work",
];

pub(crate) fn text_has_blocked_future_action_signal(lower: &str) -> bool {
    BLOCKED_FUTURE_ACTION_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
}

pub(crate) fn text_has_no_followup_justification(lower: &str) -> bool {
    NO_FOLLOWUP_JUSTIFICATION_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
}

/// True when a blocked / still-needed-work phrase co-occurs with `#id` inside
/// the same paragraph of the response. Paragraph-scoping (blank-line separated)
/// keeps an incidental blocked phrase about unrelated work — or the `### Re:`
/// heading that always names the id — from tying the signal to the directed id.
pub(crate) fn blocked_signal_tied_to_id(text: &str, id: &str) -> bool {
    let needle = format!("#{}", id.to_ascii_lowercase());
    text.split("\n\n").any(|paragraph| {
        let lower = paragraph.to_ascii_lowercase();
        lower.contains(&needle) && text_has_blocked_future_action_signal(&lower)
    })
}

/// Open (`[ ]`/gated, not done) ids that currently live in a `review`/gated
/// component. Used to confirm a directed id gated this cycle is still gated
/// (not subsequently un-gated or completed) before the blocked-closeout guard
/// fires.
pub(crate) fn open_review_ids(file: &Path) -> Result<std::collections::HashSet<String>> {
    let content = std::fs::read_to_string(file)?;
    let Ok(components) = crate::component::parse(&content) else {
        return Ok(std::collections::HashSet::new());
    };
    Ok(components
        .into_iter()
        .filter(|component| crate::component::is_review_component(&component.name))
        .flat_map(|component| {
            let (_, items, _) = crate::pending::parse_items(component.content(&content));
            items
        })
        .filter(|item| !item.is_done())
        .map(|item| crate::pending::normalize_pending_id(&item.id))
        .filter(|id| !id.is_empty())
        .collect())
}
