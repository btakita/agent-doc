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
    let mut removal_proofs: Vec<(String, &'static str)> = Vec::new();
    for id in recorded_ids {
        let norm = crate::pending::normalize_pending_id(&id);
        if norm.is_empty() {
            continue;
        }
        if still_queued.contains(&norm) {
            continue; // head preserved in the committed queue
        }
        if !open_backlog.contains(&norm) {
            if !removal_proofs.iter().any(|(existing, _)| existing == &norm) {
                removal_proofs.push((norm, "backlog_resolved_or_removed"));
            }
            continue; // backlog item resolved / removed → deletion proven
        }
        if resolved.contains(&norm) {
            if !removal_proofs.iter().any(|(existing, _)| existing == &norm) {
                removal_proofs.push((norm, "cycle_lifecycle_outcome"));
            }
            continue; // explicit lifecycle proof
        }
        if directive_targets.contains(&norm) {
            if !removal_proofs.iter().any(|(existing, _)| existing == &norm) {
                removal_proofs.push((norm, "current_directive_target"));
            }
            continue; // sibling-owned target
        }
        if !lost.iter().any(|existing| existing == &norm) {
            lost.push(norm);
        }
    }

    for (id, proof_source) in &removal_proofs {
        crate::ops_log::log_op(
            file,
            &format!(
                "queue_head_removal_guard_proof file={} removed=#{} proof_source={}",
                file.display(),
                id,
                proof_source
            ),
        );
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
            "queue_head_removal_guard_fired file={} lost={} proof_source=missing",
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
        if free_text_queue_marker_has_bare_heading_residue(&content) {
            crate::ops_log::log_op(
                file,
                &format!(
                    "free_text_queue_marker_residue_fired file={} residue=bare_heading",
                    file.display()
                ),
            );
            return Ok(GuardResult::Error(format!(
                "[session-check] INTERRUPTED: {} contains `<!-- no-free-text-queue-head-guard -->` plus a bare `###` heading, which is interrupted closeout evidence rather than committed response proof. Finish the response through `agent-doc finalize {}` or `agent-doc write --commit {}`, then run `agent-doc session-check {}`. (see #directchatpb2)",
                file.display(),
                file.display(),
                file.display(),
                file.display()
            )));
        }
        return Ok(GuardResult::None);
    }
    let Ok(components) = crate::component::parse(&content) else {
        return Ok(GuardResult::None);
    };
    let exchange_text: String = components
        .iter()
        .find(|c| c.name == "exchange")
        .map(|c| c.content(&content).to_string())
        .unwrap_or_default();
    let mut unresolved: Vec<String> = Vec::new();
    let mut response_proven_removed: Vec<String> = Vec::new();
    let mut completed_residue: Vec<String> = Vec::new();
    for head in &state.active_free_text_queue_heads {
        let normalized = head.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        // `#qimpstrike`: a recurring imperative command head (`deploy`, `commit`,
        // `push`, the `#spec-test-commit-push` preset, …) is an executable
        // directive that is valid every time it is queued. A response that echoed
        // it as a `> **Queue prompt:**` quote does NOT answer/retire a standing
        // command, so the residue guard must leave it active/drainable rather than
        // flag it as "completed queue residue." Only genuine one-time prompts are
        // residue candidates.
        if crate::queue_continuation::is_recurring_imperative_head(head) {
            continue;
        }
        let still_queued = committed_queue_contains_active_free_text_head(&content, head);
        if still_queued {
            if crate::write::free_text_head_answered_by_response(&exchange_text, head) {
                completed_residue.push(head.clone());
            }
            continue;
        }
        if crate::write::free_text_head_answered_by_response(&exchange_text, head)
            || response_head_plausibly_answers(&exchange_text, head)
        {
            response_proven_removed.push(head.clone());
            continue;
        }
        unresolved.push(head.clone());
    }
    if !completed_residue.is_empty() {
        let heads_text = completed_residue
            .iter()
            .map(|h| format!("{:?}", h))
            .collect::<Vec<_>>()
            .join("; ");
        crate::ops_log::log_op(
            file,
            &format!(
                "free_text_queue_completed_residue_guard_fired file={} residue={}",
                file.display(),
                heads_text
            ),
        );
        return Ok(GuardResult::Error(format!(
            "[session-check] INTERRUPTED: completed free-text agent:queue head(s) {heads_text} are still active in the committed queue even though exchange history contains a `Queue prompt` echo proving they were already answered — completed queue residue would re-run stale work\n[session-check] hint: remove or strike the answered head(s), then re-run `agent-doc write --commit {}`; add `<!-- no-free-text-queue-head-guard -->` only if keeping the answered row active is intentional (see #qheadresidue)",
            file.display()
        )));
    }
    if !response_proven_removed.is_empty() {
        let heads_text = response_proven_removed
            .iter()
            .map(|h| format!("{:?}", h))
            .collect::<Vec<_>>()
            .join("; ");
        crate::ops_log::log_op(
            file,
            &format!(
                "free_text_queue_head_provenance_proof file={} removed={} proof_source=committed_response",
                file.display(),
                heads_text
            ),
        );
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

fn normalized_free_text_queue_head_identity(text: &str) -> String {
    crate::queue::strip_priority_markers(text)
        .trim()
        .to_ascii_lowercase()
}

fn committed_queue_contains_active_free_text_head(content: &str, head: &str) -> bool {
    let Ok(components) = crate::component::parse(content) else {
        return false;
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return false;
    };
    let Ok(entries) = crate::queue::parse(queue.content(content)) else {
        return false;
    };
    let target = normalized_free_text_queue_head_identity(head);
    if target.is_empty() {
        return false;
    }
    crate::queue::prompts(&entries).into_iter().any(|prompt| {
        let text = prompt.text.trim();
        crate::write::queue_prompt_text_is_free_text(content, text)
            && normalized_free_text_queue_head_identity(text) == target
    })
}

pub(crate) fn free_text_queue_marker_has_bare_heading_residue(content: &str) -> bool {
    content.contains("<!-- no-free-text-queue-head-guard -->")
        && content.lines().any(|line| line.trim() == "###")
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    fn make_doc(root: &Path, content: &str) -> PathBuf {
        fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("doc.md");
        fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();
        doc
    }

    fn mark_cycle_committed(doc: &Path, preflight: &str, committed: &str) {
        crate::cycle_state::start_preflight(doc, Some(preflight), Some(preflight)).unwrap();
        fs::write(doc, committed).unwrap();
        crate::snapshot::save(doc, committed).unwrap();
        crate::cycle_state::mark_committed(doc, "commit_success", Some(committed), Some(committed))
            .unwrap();
    }

    fn run_context(doc: &Path, content: &str) -> crate::graph::RunContext {
        let rc = crate::graph::RunContext::new(doc.to_path_buf());
        rc.set_doc_content(content.to_string());
        rc
    }

    fn ops_log(root: &Path) -> String {
        fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap_or_default()
    }

    #[test]
    fn queue_head_removal_guard_logs_proof_source_for_authorized_id_removals() {
        let tmp = tempfile::TempDir::new().unwrap();
        let preflight = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n### Re: prior\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#done] done this cycle\n",
            "- [ ] [#keep] still queued\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#done]\n",
            "- do [#resolved]\n",
            "- do [#keep]\n",
            "<!-- /agent:queue -->\n",
        );
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n### Re: do #done\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#done] done this cycle\n",
            "- [ ] [#keep] still queued\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#keep]\n",
            "<!-- /agent:queue -->\n",
        );
        let doc = make_doc(tmp.path(), preflight);
        mark_cycle_committed(&doc, preflight, committed);
        crate::cycle_state::record_pending_done_ids(&doc, &["done".to_string()]).unwrap();

        let rc = run_context(&doc, committed);
        assert!(
            matches!(
                check_queue_head_removal_guard(&doc, &rc).unwrap(),
                GuardResult::None
            ),
            "authorized queue-head removals should not interrupt"
        );
        let log = ops_log(tmp.path());
        assert!(
            log.contains("removed=#done proof_source=cycle_lifecycle_outcome"),
            "done removal proof should name the id and source:\n{log}"
        );
        assert!(
            log.contains("removed=#resolved proof_source=backlog_resolved_or_removed"),
            "resolved backlog removal proof should name the id and source:\n{log}"
        );
    }

    #[test]
    fn free_text_queue_head_guard_logs_response_proof_source_for_removed_head() {
        let tmp = tempfile::TempDir::new().unwrap();
        let preflight = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n### Re: prior\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- Please explain the churn\n",
            "<!-- /agent:queue -->\n",
        );
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: Please explain the churn\n\n",
            "The churn comes from stale queue convergence.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n",
        );
        let doc = make_doc(tmp.path(), preflight);
        mark_cycle_committed(&doc, preflight, committed);

        let rc = run_context(&doc, committed);
        assert!(
            matches!(
                check_free_text_queue_head_provenance(&doc, &rc).unwrap(),
                GuardResult::None
            ),
            "answered free-text queue head should not interrupt"
        );
        let log = ops_log(tmp.path());
        assert!(
            log.contains("free_text_queue_head_provenance_proof")
                && log.contains("Please explain the churn")
                && log.contains("proof_source=committed_response"),
            "free-text removal proof should name the head and source:\n{log}"
        );
    }

    #[test]
    fn completed_free_text_queue_residue_guard_fires_for_answered_active_head() {
        // A genuine ONE-TIME prompt head that the response answered, but which is
        // still active in the committed queue, is completed residue and must fire
        // the `#qheadresidue` guard. (A recurring-imperative command head is the
        // exception — see the `#qimpstrike` test below.)
        let tmp = tempfile::TempDir::new().unwrap();
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\npending_done_guard: warn\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: explain the queue churn -- gpt-5\n\n",
            "> **Queue prompt:**\n>\n> explain the queue churn\n\n",
            "The churn comes from stale convergence.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- explain the queue churn\n",
            "<!-- /agent:queue -->\n",
        );
        let doc = make_doc(tmp.path(), committed);
        mark_cycle_committed(&doc, committed, committed);

        let rc = run_context(&doc, committed);
        match check_free_text_queue_head_provenance(&doc, &rc).unwrap() {
            GuardResult::Error(message) => {
                assert!(message.contains("completed free-text"), "got: {message}");
                assert!(message.contains("#qheadresidue"), "got: {message}");
                assert!(message.contains("explain the queue churn"), "got: {message}");
            }
            other => panic!("completed queue residue must interrupt, got {other:?}"),
        }
        let log = ops_log(tmp.path());
        assert!(
            log.contains("free_text_queue_completed_residue_guard_fired"),
            "residue guard should log the proved completed head:\n{log}"
        );
    }

    #[test]
    fn residue_guard_exempts_recurring_imperative_deploy_head() {
        // #qimpstrike: a recurring-imperative command head (`deploy`) is an
        // executable directive that stays valid every cycle. A response that
        // echoed it as a `> **Queue prompt:**` quote does NOT retire a standing
        // `deploy` directive, so the `#qheadresidue` residue guard must NOT fire
        // — the head remains active/drainable for the next dispatch.
        let tmp = tempfile::TempDir::new().unwrap();
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\npending_done_guard: warn\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: deploy -- gpt-5\n\n",
            "> **Queue prompt:**\n>\n> deploy\n\n",
            "Deployed successfully.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- deploy\n",
            "<!-- /agent:queue -->\n",
        );
        let doc = make_doc(tmp.path(), committed);
        mark_cycle_committed(&doc, committed, committed);

        let rc = run_context(&doc, committed);
        assert!(
            matches!(
                check_free_text_queue_head_provenance(&doc, &rc).unwrap(),
                GuardResult::None
            ),
            "recurring-imperative `deploy` head must not be struck as completed residue"
        );
        let log = ops_log(tmp.path());
        assert!(
            !log.contains("free_text_queue_completed_residue_guard_fired"),
            "residue guard must not fire for a recurring-imperative head:\n{log}"
        );
    }
}
