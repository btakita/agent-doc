use super::*;

pub(crate) fn check_blocked_closeout_followup_guard(
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
    if state.expect_done_or_gate_ids.is_empty()
        || state.pending_gated_ids.is_empty()
        || state.is_open()
    {
        return Ok(GuardResult::None);
    }
    // A new follow-up backlog/review item was captured this cycle — satisfied.
    if state.pending_added_this_cycle {
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
        .contains("<!-- no-blocked-followup-guard -->")
        || capture
            .response_body
            .contains("<!-- no-pending-done-guard -->")
    {
        return Ok(GuardResult::None);
    }

    let text = response_text_for_guards(&capture.response_body);
    let lower = text.to_ascii_lowercase();
    if !text_has_blocked_future_action_signal(&lower) {
        return Ok(GuardResult::None);
    }
    if text_has_no_followup_justification(&lower) {
        return Ok(GuardResult::None);
    }

    let kept_open: std::collections::HashSet<String> = state
        .pending_kept_open_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();
    let done: std::collections::HashSet<String> = state
        .pending_done_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();
    let gated: std::collections::HashSet<String> = state
        .pending_gated_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();
    let still_gated = open_review_ids(file)?;

    let mut unresolved: Vec<String> = Vec::new();
    for id in state
        .expect_done_or_gate_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
    {
        if kept_open.contains(&id) || done.contains(&id) {
            continue;
        }
        if !gated.contains(&id) || !still_gated.contains(&id) {
            continue;
        }
        // Tie the blocked signal to the directed id (same paragraph) so an
        // incidental blocked phrase about unrelated work does not fire.
        if !blocked_signal_tied_to_id(&text, &id) {
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
    let edit_hint = unresolved
        .iter()
        .map(|id| format!("--pending-edit \"{}=<remaining next action>\"", id))
        .collect::<Vec<_>>()
        .join(" ");
    let add_after_hint = unresolved
        .first()
        .map(|id| format!("--pending-add-after {} \"<id>=<concrete next step>\"", id))
        .unwrap_or_default();
    let repair = format!(
        "agent-doc write {} {} --pending-only --commit",
        file.display(),
        edit_hint
    );
    let warn_line = format!(
        "[session-check] warn: `do #id` closeout reported blocked / still-needed work but gated tracked target {} out of agent:backlog with no kept-open edit, new follow-up item, or explicit no-follow-up justification — the remaining steps live only in prose",
        ids
    );

    crate::ops_log::log_op(
        file,
        &format!(
            "blocked_closeout_followup_guard_fired file={} unresolved={}",
            file.display(),
            unresolved.join(",")
        ),
    );

    Ok(match mode {
        crate::frontmatter::PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!(
                "[session-check] hint: keep the work tracked with `{}`, split a new follow-up via `{}`, add an explicit \"no additional backlog follow-up is needed because ...\" phrase for a true review-only gate, or add `<!-- no-blocked-followup-guard -->`",
                repair, add_after_hint
            ),
        ]),
        crate::frontmatter::PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: keep the work tracked with `{}`, split a new follow-up via `{}`, add an explicit \"no additional backlog follow-up is needed because ...\" phrase for a true review-only gate, or set pending_done_guard = \"warn\" to downgrade",
            warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
            repair,
            add_after_hint
        )),
        crate::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

/// `#gated-followup-split-enforcement`: when a directed `do [#id]` cycle keeps a
/// multi-phase item open (via `--pending-edit` / `--review-edit` /
/// `--pending-gate`) whose body enumerates several gated/remaining phases but
/// never breaks them out into discrete child backlog IDs, the deferred phases
/// stay buried in one parent's narrowed description and are not independently
/// trackable or queueable. Advise splitting each phase into its own child ID
/// (sibling of `#blocked-closeout-followup-capture` and the SKILL "one backlog
/// ID per actionable phase" rule).
///
/// Warn-first advisory only — it never blocks closeout. Suppressible via a
/// `<!-- no-gated-phase-split-guard -->` response marker.
pub(crate) fn check_gated_phase_split_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.expect_done_or_gate_ids.is_empty() || state.is_open() {
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
        .contains("<!-- no-gated-phase-split-guard -->")
    {
        return Ok(GuardResult::None);
    }

    // Items kept open this cycle (`--pending-edit` / `--review-edit` /
    // `--pending-gate` all feed `pending_kept_open_ids`) that were also the
    // directed targets — the parent items at risk of burying gated phases.
    let kept_open: std::collections::HashSet<String> = state
        .pending_kept_open_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();
    if kept_open.is_empty() {
        return Ok(GuardResult::None);
    }
    let directed: std::collections::HashSet<String> = state
        .expect_done_or_gate_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();

    // Phase 6 (#lr-content-6): cached content + parsed components.
    let content = rc.doc_content();
    let components = rc.components();
    let mut flagged: Vec<String> = Vec::new();
    for component in components.iter() {
        let trackable = crate::component::is_backlog_component(&component.name)
            || crate::component::is_review_component(&component.name);
        if !trackable {
            continue;
        }
        let (_, items, _) = crate::pending::parse_items(component.content(&content));
        for item in items {
            if item.is_done() {
                continue;
            }
            let id = crate::pending::normalize_pending_id(&item.id);
            if id.is_empty() || !kept_open.contains(&id) || !directed.contains(&id) {
                continue;
            }
            let body = format!("{} {}", item.text, item.continuation);
            if body_enumerates_multiple_gated_phases(&body)
                && !body_already_split_into_child_ids(&body, &id)
                && !flagged.iter().any(|existing| existing == &id)
            {
                flagged.push(id);
            }
        }
    }
    if flagged.is_empty() {
        return Ok(GuardResult::None);
    }

    let ids = flagged
        .iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(", ");
    let add_after_hint = flagged
        .first()
        .map(|id| format!("--pending-add-after {id} \"<child-id>=<one phase scope>\""))
        .unwrap_or_default();

    crate::ops_log::log_op(
        file,
        &format!(
            "gated_phase_split_guard_fired file={} flagged={}",
            file.display(),
            flagged.join(",")
        ),
    );

    Ok(GuardResult::Warn(vec![
        format!(
            "[session-check] warn: kept-open tracked item {ids} enumerates multiple gated/remaining phases in its body but does not break them out into discrete child backlog IDs — the deferred phases are not independently trackable or queueable"
        ),
        format!(
            "[session-check] hint: split each gated phase into its own child id (e.g. `agent-doc write {} {} --pending-only --commit`), keeping the parent as context, or add `<!-- no-gated-phase-split-guard -->` if the phases are intentionally one unit",
            file.display(),
            add_after_hint
        ),
    ]))
}

/// True when a kept-open item body enumerates multiple gated/remaining phases:
/// the word "phase" appears, at least two short parenthesized phase markers
/// (`(1)`, `(2a)`, `(2b)`, `(3)`, ...) are present, and a gating/remaining
/// signal frames them as deferred work.
pub(crate) fn body_enumerates_multiple_gated_phases(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    if !lower.contains("phase") {
        return false;
    }
    let gating = [
        "gated",
        "remaining",
        "live-verify",
        "live verify",
        "awaiting",
        "still needs",
        "not yet",
    ];
    if !gating.iter().any(|signal| lower.contains(signal)) {
        return false;
    }
    count_phase_markers(body) >= 2
}

/// Count distinct short parenthesized phase markers like `(1)`, `(2a)`, `(2b)`,
/// `(3)`. Requires 1-2 digits optionally followed by 1-2 ASCII lowercase letters
/// so dates and commit hashes (`(2026-05-31)`, `(submodule 407b0825)`) are not
/// mistaken for phase markers.
pub(crate) fn count_phase_markers(body: &str) -> usize {
    static MARKER: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\((\d{1,2}[a-z]{0,2})\)").unwrap());
    let mut seen = std::collections::HashSet::new();
    for cap in MARKER.captures_iter(body) {
        seen.insert(cap[1].to_string());
    }
    seen.len()
}

/// True when the body already references at least two discrete child ids other
/// than its own (and other than the ubiquitous `#agent-doc-bug` preset tag) —
/// i.e. the phases were already broken out into independently trackable ids, so
/// the split advisory should stay quiet.
pub(crate) fn body_already_split_into_child_ids(body: &str, own_id: &str) -> bool {
    static ID_REF: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"#([a-z0-9][a-z0-9-]*)").unwrap());
    let mut others = std::collections::HashSet::new();
    for cap in ID_REF.captures_iter(body) {
        let id = crate::pending::normalize_pending_id(&cap[1]);
        if !id.is_empty() && id != own_id && id != "agent-doc-bug" {
            others.insert(id);
        }
    }
    others.len() >= 2
}

/// Substep-completion phrases that evidence partial progress in a queue audit.
pub(crate) const QUEUE_AUDIT_SUBSTEP_COMPLETE_PHRASES: &[&str] = &[
    "is complete",
    "was complete",
    "are complete",
    "were complete",
    "is done",
    "was done",
    "was clean",
    "is clean",
    "is current",
    "are current",
    "passed",
    "verified clean",
    "already complete",
];

/// `#queue-audit-partial-completion`: detect a queue-completion audit response
/// that collapses meaningful partial progress into a blanket "none complete."
///
/// A queue audit ("which queue items are complete?") should classify each row as
/// complete / partially complete / not-started, naming completed substeps and the
/// exact remaining condition — not answer "none are complete" just because every
/// row still has one remaining action. This warn-first guard fires only on the
/// clearest collapse signal: the response is about the queue, makes a blanket
/// none-complete claim, shows at least two distinct substep-completion signals,
/// and never frames anything as "partial." It is WARN-only (never blocks
/// closeout) and suppressed by a `<!-- no-queue-audit-guard -->` marker.
///
/// The richer per-row state table is response guidance (a natural-language
/// judgment that lives in the skill/spec contract, per the binary-vs-skill rule),
/// so the binary only flags the unambiguous collapse rather than trying to
/// classify free-text rows itself.
pub(crate) fn check_queue_audit_partial_completion_guard(file: &Path) -> Result<GuardResult> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
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
        .contains("<!-- no-queue-audit-guard -->")
    {
        return Ok(GuardResult::None);
    }

    let text = response_text_for_guards(&capture.response_body);
    let lower = text.to_ascii_lowercase();
    if !queue_audit_collapses_partial_completion(&lower) {
        return Ok(GuardResult::None);
    }

    crate::ops_log::log_op(
        file,
        &format!(
            "queue_audit_partial_completion_guard_fired file={}",
            file.display()
        ),
    );

    Ok(GuardResult::Warn(vec![
        "[session-check] warn: this queue-completion audit reports the queue as not complete while also citing several completed substeps, but never classifies any row as partially complete — meaningful partial progress is collapsed into \"none complete\"".to_string(),
        "[session-check] hint: classify each queue row as complete / partially complete / not-started, naming the completed substeps and the exact remaining condition for partial rows; recommend splitting a row with multiple gateable phases. Add `<!-- no-queue-audit-guard -->` if the all-or-none framing is intentional.".to_string(),
    ]))
}

/// True when a queue-audit response collapses partial completion: it is about the
/// queue, makes a blanket none-complete claim, shows >=2 distinct substep
/// completions, and never frames anything as "partial."
pub(crate) fn queue_audit_collapses_partial_completion(lower: &str) -> bool {
    if !lower.contains("queue") {
        return false;
    }
    // Already broke it down — not a collapse.
    if lower.contains("partial") {
        return false;
    }
    if !queue_audit_has_none_complete_claim(lower) {
        return false;
    }
    let substep_completions = QUEUE_AUDIT_SUBSTEP_COMPLETE_PHRASES
        .iter()
        .filter(|phrase| lower.contains(*phrase))
        .count();
    substep_completions >= 2
}

/// A blanket "none / not ... complete" claim about the queue items.
pub(crate) fn queue_audit_has_none_complete_claim(lower: &str) -> bool {
    static NONE_COMPLETE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        // "none of the queue items is/are (fully) complete", "no items are
        // complete", "none are fully complete", etc. — a none/no quantifier
        // within a short span before a complete/completed token.
        regex::Regex::new(r"\b(none|no)\b[^.\n]{0,60}?\bcomplet(e|ed)\b").unwrap()
    });
    NONE_COMPLETE.is_match(lower)
}

pub(crate) fn single_open_review_item_id(file: &Path) -> Result<Option<String>> {
    let content = std::fs::read_to_string(file)?;
    let Ok(components) = crate::component::parse(&content) else {
        return Ok(None);
    };
    let ids = components
        .into_iter()
        .filter(|component| crate::component::is_review_component(&component.name))
        .flat_map(|component| {
            let (_, items, _) = crate::pending::parse_items(component.content(&content));
            items
        })
        .filter(|item| !item.is_done())
        .map(|item| item.id)
        .collect::<Vec<_>>();
    if ids.len() == 1 {
        Ok(ids.into_iter().next())
    } else {
        Ok(None)
    }
}

pub(crate) fn phase_name(phase: crate::cycle_state::CyclePhase) -> &'static str {
    match phase {
        crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
        crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
        crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
        crate::cycle_state::CyclePhase::Committed => "committed",
        crate::cycle_state::CyclePhase::Abandoned => "abandoned",
    }
}

pub(crate) fn detect_active_session_post_commit_drift(file: &Path) -> Result<Option<String>> {
    let Some(session) = crate::codex_hook::load_active_session_for_current_file(file)? else {
        return Ok(None);
    };
    let Some(snapshot) = crate::snapshot::load(file)? else {
        return Ok(None);
    };
    let current = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    if current == snapshot {
        return Ok(None);
    }
    if crate::git::normalize_committed_exchange_artifacts(&current)
        == crate::git::normalize_committed_exchange_artifacts(&snapshot)
    {
        return Ok(None);
    }

    let prompt_marker = detect_unstarted_prompt_bearing_diff(file)?;
    if prompt_marker.is_none()
        && active_session_drift_is_only_exchange_or_backlog_metadata(&snapshot, &current)
    {
        return Ok(None);
    }
    if prompt_marker.is_none() && promptless_comment_only_drift(&snapshot, &current) {
        return Ok(None);
    }
    if prompt_marker.is_none() && exchange_only_promptless_content_drift(&snapshot, &current) {
        return Ok(None);
    }
    let prompt_preview = session
        .last_prompt
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("agent-doc session");
    let prompt_preview = prompt_preview.trim();

    let detail = match prompt_marker {
        Some(marker) => format!(
            "{}; active_session={} turn={} prompt={}",
            marker, session.session_id, session.last_turn_id, prompt_preview
        ),
        None => format!(
            "active_session={} turn={} prompt={}",
            session.session_id, session.last_turn_id, prompt_preview
        ),
    };
    Ok(Some(detail))
}

pub(crate) fn detect_uncommitted_exchange_drift(file: &Path) -> Result<Option<String>> {
    let Some(snapshot) = crate::snapshot::load(file)? else {
        return Ok(None);
    };
    let current = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    if current == snapshot {
        return Ok(None);
    }
    let norm_current = crate::git::normalize_committed_exchange_artifacts(&current);
    let norm_snapshot = crate::git::normalize_committed_exchange_artifacts(&snapshot);
    if norm_current == norm_snapshot {
        return Ok(None);
    }
    if !exchange_has_new_appended_content(&norm_snapshot, &norm_current) {
        return Ok(None);
    }
    let prompt_marker = detect_unstarted_prompt_bearing_diff(file)?;
    let detail = match prompt_marker {
        Some(marker) => format!(
            "uncommitted working tree drift beyond snapshot with exchange changes; {}",
            marker
        ),
        None => "uncommitted working tree drift beyond snapshot with exchange changes".to_string(),
    };
    Ok(Some(detail))
}

pub(crate) fn exchange_has_new_appended_content(snapshot: &str, current: &str) -> bool {
    let Some(snapshot_exchange) = extract_normalized_exchange_body(snapshot) else {
        return false;
    };
    let Some(current_exchange) = extract_normalized_exchange_body(current) else {
        return false;
    };
    if current_exchange == snapshot_exchange {
        return false;
    }
    let snapshot_lines: Vec<&str> = snapshot_exchange.lines().collect();
    let current_lines: Vec<&str> = current_exchange.lines().collect();
    if current_lines.len() <= snapshot_lines.len() {
        return false;
    }
    for (i, line) in snapshot_lines.iter().enumerate() {
        if current_lines.get(i) != Some(line) {
            return false;
        }
    }
    let appended: String = current_lines[snapshot_lines.len()..].join("\n");
    if appended
        .lines()
        .map(str::trim)
        .any(is_exchange_response_heading)
    {
        return true;
    }
    if appended
        .lines()
        .any(crate::diff::text_line_looks_like_prompt_target)
    {
        return false;
    }
    true
}

pub(crate) fn extract_normalized_exchange_body(doc: &str) -> Option<String> {
    let (_, body) = crate::frontmatter::parse(doc).ok()?;
    let components = crate::component::parse(body).ok()?;
    for component in &components {
        if component.name == "exchange" {
            return Some(component.content(body).to_string());
        }
    }
    None
}

pub(crate) fn exchange_only_promptless_content_drift(snapshot: &str, current: &str) -> bool {
    if snapshot == current {
        return true;
    }
    let Some(snapshot_masked) = mask_exchange_component_content(snapshot) else {
        return false;
    };
    let Some(current_masked) = mask_exchange_component_content(current) else {
        return false;
    };
    crate::git::normalize_transient_agent_doc_markers(&snapshot_masked)
        == crate::git::normalize_transient_agent_doc_markers(&current_masked)
}

pub(crate) fn active_session_drift_is_only_exchange_or_backlog_metadata(
    snapshot: &str,
    current: &str,
) -> bool {
    let Some(snapshot_masked) = mask_components_by_name(snapshot, &["exchange", "backlog"]) else {
        return false;
    };
    let Some(current_masked) = mask_components_by_name(current, &["exchange", "backlog"]) else {
        return false;
    };
    crate::git::normalize_transient_agent_doc_markers(&snapshot_masked)
        == crate::git::normalize_transient_agent_doc_markers(&current_masked)
}

pub(crate) fn promptless_comment_only_drift(snapshot: &str, current: &str) -> bool {
    if snapshot == current {
        return true;
    }
    crate::git::normalize_transient_agent_doc_markers(&crate::diff::strip_comments(snapshot))
        == crate::git::normalize_transient_agent_doc_markers(&crate::diff::strip_comments(current))
}

pub(crate) fn mask_exchange_component_content(doc: &str) -> Option<String> {
    mask_components_by_name(doc, &["exchange"])
}

pub(crate) fn mask_components_by_name(doc: &str, names: &[&str]) -> Option<String> {
    let components = crate::component::parse(doc).ok()?;
    let mut masked = doc.to_string();
    let mut saw_target = false;
    for component in components.iter().rev() {
        if !names.contains(&component.name.as_str()) {
            continue;
        }
        saw_target = true;
        masked.replace_range(component.open_end..component.close_start, "\n");
    }
    saw_target.then_some(masked)
}

pub(crate) fn open_cycle_message(
    file: &Path,
    state: &crate::cycle_state::CycleState,
) -> Result<String> {
    let ipc_hint = latest_ipc_proof_diagnostic_hint(file)?
        .map(|hint| format!(" {hint}"))
        .unwrap_or_default();
    if state.last_event.starts_with("direct_invocation_timeout")
        || state
            .last_event
            .starts_with("recursive_direct_invocation_blocked")
    {
        return Ok(format!(
            "[session-check] INTERRUPTED: cycle `{}` is still `{}` ({}) — direct invocation did not reach response capture. If the owning pane is now idle but the document still reports busy, reconcile it without killing the pane via `agent-doc session status {}` (or `agent-doc session clear {}`). Otherwise retry from outside the managed pane, restart the owner with `agent-doc start {}`, or abandon the stale cycle only after confirming no response exists.{}",
            state.cycle_id,
            phase_name(state.phase),
            state.last_event,
            state.file,
            state.file,
            state.file,
            ipc_hint
        ));
    }
    let detail = match state.phase {
        crate::cycle_state::CyclePhase::PreflightStarted => {
            "cycle started but no write/commit followed"
        }
        crate::cycle_state::CyclePhase::ResponseCaptured => {
            "response was captured but no write/commit followed"
        }
        crate::cycle_state::CyclePhase::WriteApplied => {
            "response write landed but no terminal commit followed"
        }
        crate::cycle_state::CyclePhase::Committed => "no terminal commit followed",
        crate::cycle_state::CyclePhase::Abandoned => "cycle was abandoned",
    };
    Ok(format!(
        "[session-check] INTERRUPTED: cycle `{}` is still `{}` ({}) — {}.{}",
        state.cycle_id,
        phase_name(state.phase),
        state.last_event,
        detail,
        ipc_hint
    ))
}

pub(crate) fn open_cycle_manual_patchback_message(
    file: &Path,
    state: &crate::cycle_state::CycleState,
) -> Result<Option<String>> {
    if !matches!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted
    ) {
        return Ok(None);
    }
    let Some(marker) = detect_bypassed_response_write(file)? else {
        return Ok(None);
    };
    Ok(Some(format!(
        "[session-check] INTERRUPTED: cycle `{}` is still `{}` ({}) — found visible response patchback {} that is still outside the commit boundary. This looks like a manual repair that stopped before commit; finish it with `agent-doc write --commit {}` if you still have the response body, or commit the repaired document manually once the response is correct.",
        state.cycle_id,
        phase_name(state.phase),
        state.last_event,
        marker,
        file.display()
    )))
}

/// Return the message portion of the last non-empty line in `ops.log`,
/// stripped of the `[epoch_secs] ` timestamp prefix.
///
/// Returns `Ok(None)` when the log file is missing or empty.
pub fn last_ops_event(file: &Path) -> Result<Option<String>> {
    let canonical = match file.canonicalize() {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let Some(project_root) = crate::snapshot::find_project_root(&canonical) else {
        return Ok(None);
    };
    let log_path = project_root.join(".agent-doc/logs/ops.log");
    let Some(content) = crate::fs_util::read_optional_text(&log_path)? else {
        return Ok(None);
    };
    let canonical_display = canonical.display().to_string();
    let requested_display = file.display().to_string();
    let last = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rfind(|line| {
            line.contains(&format!("file={canonical_display}"))
                || line.contains(&format!("file={requested_display}"))
        })
        .or_else(|| content.lines().rfind(|l| !l.trim().is_empty()))
        .map(|l| strip_timestamp_prefix(l).to_string());
    Ok(last)
}

pub fn latest_ipc_proof_diagnostic(file: &Path) -> Result<Option<String>> {
    let canonical = match file.canonicalize() {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let Some(project_root) = crate::snapshot::find_project_root(&canonical) else {
        return Ok(None);
    };
    let log_path = project_root.join(".agent-doc/logs/ops.log");
    let Some(content) = crate::fs_util::read_optional_text(&log_path)? else {
        return Ok(None);
    };
    let canonical_display = canonical.display().to_string();
    let requested_display = file.display().to_string();
    Ok(content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .map(strip_timestamp_prefix)
        .find(|event| {
            event.starts_with(IPC_PROOF_INSUFFICIENT_EVENT)
                && (event.contains(&format!("file={canonical_display}"))
                    || event.contains(&format!("file={requested_display}")))
        })
        .map(str::to_string))
}

pub fn latest_ipc_proof_diagnostic_hint(file: &Path) -> Result<Option<String>> {
    Ok(latest_ipc_proof_diagnostic(file)?
        .map(|event| format!("latest IPC proof diagnostic: {event}")))
}

/// Strip a leading `[NNN] ` timestamp prefix from a log line.
pub(crate) fn strip_timestamp_prefix(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix('[')
        && let Some(close) = rest.find("] ")
    {
        return &rest[close + 2..];
    }
    line
}

pub fn detect_write_completed_commit_missing(file: &Path) -> Result<Option<String>> {
    Ok(last_ops_event(file)?.filter(|event| is_write_completed_commit_missing_event(event)))
}

pub(crate) fn is_write_completed_commit_missing_event(event: &str) -> bool {
    event.starts_with(IPC_WRITE_CONSUMED_EVENT) || event.starts_with(SNAPSHOT_SAVED_FILE_IPC_EVENT)
}

pub(crate) fn event_name(event: &str) -> &str {
    event.split_whitespace().next().unwrap_or(event)
}

pub fn detect_bypassed_response_write(file: &Path) -> Result<Option<String>> {
    let Some(snapshot) = crate::snapshot::load(file)? else {
        return Ok(None);
    };
    let current = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    Ok(detect_bypassed_response_write_between(&snapshot, &current))
}

pub fn detect_bypassed_response_write_between(
    snapshot_doc: &str,
    current_doc: &str,
) -> Option<String> {
    // Normalize transient markers before comparison — (HEAD) annotations and
    // boundary IDs legitimately differ between snapshot (clean) and working tree
    // (preserves HEAD). Without this, preserved (HEAD) markers cause false-positive
    // "direct response patchback" detection.
    let norm = |s: &str| crate::git::normalize_transient_agent_doc_markers(s);
    let snap_norm = norm(snapshot_doc);
    let cur_norm = norm(current_doc);
    if cur_norm == snap_norm {
        return None;
    }
    if !has_new_response_heading_marker(&snap_norm, &cur_norm) {
        return None;
    }

    let diff_text = crate::diff::unified_diff_from_contents(&snap_norm, &cur_norm)?;

    let diff = similar::TextDiff::from_lines(&snap_norm, &cur_norm);
    for change in diff.iter_all_changes() {
        if change.tag() != similar::ChangeTag::Insert {
            continue;
        }
        let trimmed = change.value().trim();
        if is_binary_authored_recovery_diagnostic_heading(trimmed) {
            continue;
        }
        if trimmed.starts_with("### Re:") || trimmed == "## Assistant" {
            if let Some(bare_target) =
                first_bare_prompt_prefix_target_before_marker(&diff_text, trimmed)
            {
                return Some(format!(
                    "{} (bare prompt target missing `❯ `: {})",
                    trimmed, bare_target
                ));
            }
            return Some(trimmed.to_string());
        }
    }
    None
}

pub(crate) fn first_bare_prompt_prefix_target_before_marker(
    diff_text: &str,
    marker: &str,
) -> Option<String> {
    let mut prefix_diff = String::new();
    for line in diff_text.lines() {
        if line
            .strip_prefix('+')
            .is_some_and(|added| added.trim() == marker)
        {
            break;
        }
        prefix_diff.push_str(line);
        prefix_diff.push('\n');
    }
    crate::diff::first_bare_prompt_prefix_target(&prefix_diff)
}

pub(crate) fn has_new_response_heading_marker(snapshot_doc: &str, current_doc: &str) -> bool {
    use std::collections::BTreeMap;

    fn marker_counts(doc: &str) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for line in doc.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("### Re:") || trimmed == "## Assistant" {
                *counts.entry(trimmed.to_string()).or_insert(0) += 1;
            }
        }
        counts
    }

    let snapshot_counts = marker_counts(snapshot_doc);
    let current_counts = marker_counts(current_doc);
    current_counts
        .into_iter()
        .any(|(marker, count)| count > snapshot_counts.get(&marker).copied().unwrap_or(0))
}

pub fn is_exchange_response_heading(trimmed: &str) -> bool {
    trimmed == "## Assistant"
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
        || trimmed.starts_with("###### Re:")
}

/// Binary-authored interrupted-cycle recovery diagnostics are appended by
/// `preflight::format_ipc_dogfood_note` with a `### Re:` heading so the diff
/// classifier sees a `RecoveryArtifact` (never a user `PromptTarget`). That same
/// `### Re:` would otherwise trip this direct-patchback detector, wedging the
/// cycle in an append -> flag -> refuse -> re-append loop. Exempt the known
/// recovery-diagnostic shape (`#ipc-recovery-diagnostic-patchback`).
pub(crate) fn is_binary_authored_recovery_diagnostic_heading(trimmed: &str) -> bool {
    (trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:"))
        && trimmed.contains("interrupted-cycle recovery")
}

/// `#prompt-preempts-auto-queue`: snapshot-independent detection of a live
/// unresolved user prompt in `agent:exchange`. A prompt is unresolved when there
/// is user-authored, non-comment text after the latest `agent:boundary` marker
/// in the exchange and no `### Re:` response heading follows it in that tail
/// segment. Unlike the snapshot-diff path, this fires even when the prompt was
/// already baselined into the snapshot (so the ordinary diff sees only queue
/// bookkeeping). Returns the joined prompt text, or `None` when the tail is
/// empty or already answered.
pub fn unresolved_exchange_prompt(file: &Path) -> Result<Option<String>> {
    let content = std::fs::read_to_string(file)?;
    Ok(unresolved_exchange_prompt_in_content(&content))
}

pub(crate) fn unresolved_exchange_prompt_in_content(content: &str) -> Option<String> {
    let components = crate::component::parse(content).ok()?;
    let exchange = components.iter().find(|c| c.name == "exchange")?;
    let body = exchange.content(content);
    let lines: Vec<&str> = body.lines().collect();

    // The latest boundary marks the end of the last committed/answered segment;
    // everything after it is the new, not-yet-answered tail.
    let tail_start = lines
        .iter()
        .rposition(|line| line.trim().starts_with("<!-- agent:boundary:"))
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let tail = &lines[tail_start..];

    // `#prompt-preempts-auto-queue` / `#queue-continuation-buries-prompt`: a
    // response heading means the prompt *above it* was answered — but a
    // queue-continuation response (`### Re: do [#id]` / `### Re: re [#id]`)
    // answers a queue/backlog item, NOT a free-text user prompt. When the only
    // response after a free-text prompt is a queue continuation, the prompt is
    // still unresolved; the queue continuation must not let the boundary bury it.
    // Scan only the prompt region up to the FIRST response heading so a queue
    // continuation's own response body is never mistaken for prompt text.
    let first_response_idx = tail
        .iter()
        .position(|line| is_exchange_response_heading(line.trim()));
    if let Some(idx) = first_response_idx {
        let heading = tail[idx].trim();
        if !is_queue_continuation_response_heading(heading) {
            // A genuine free-text answer resolves the prompt.
            return None;
        }
        // Queue-continuation response — does not answer a free-text prompt.
    }
    let prompt_region = match first_response_idx {
        Some(idx) => &tail[..idx],
        None => tail,
    };

    let prompt_lines: Vec<String> = prompt_region
        .iter()
        .map(|line| line.trim())
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("<!--")
                && !line.starts_with("-->")
                && !is_exchange_response_heading(line)
                // `#ipcproofnostall`: a binary-authored interrupted-cycle
                // IPC-proof recovery diagnostic line (the structured
                // `ipc_proof_insufficient ... invariant=... recovery=...` event or
                // its literal self-description) is not a user prompt, even when a
                // post-commit worktree corruption strips its `### Re:` heading and
                // fence and leaves the bare line in the exchange tail. Match is
                // token-specific so a real prompt mentioning IPC/drift is kept.
                && !crate::diff::line_is_binary_authored_ipc_proof_diagnostic(line)
                // `#provauth3`: a binary-authored compaction Session Summary line
                // (heading / archive pointer / archived-topic item) is not a user
                // prompt, even when an earlier content-inference repair pass stamped
                // it with a `❯` prefix. Origin is known (the binary authored the
                // compaction), so it must never INTERRUPT closeout as an unresolved
                // prompt-only tail.
                && !crate::diff::line_is_binary_authored_compact_summary(line)
        })
        .map(normalized_prompt_for_match)
        .filter(|line| !line.is_empty())
        .collect();
    if prompt_lines.is_empty() {
        return None;
    }
    Some(prompt_lines.join("\n"))
}

pub(crate) fn exchange_tail_has_response_heading(file: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(file) else {
        return false;
    };
    let Ok(components) = crate::component::parse(&content) else {
        return false;
    };
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return false;
    };
    let body = exchange.content(&content);
    let lines: Vec<&str> = body.lines().collect();
    let tail_start = lines
        .iter()
        .rposition(|line| line.trim().starts_with("<!-- agent:boundary:"))
        .map(|idx| idx + 1)
        .unwrap_or(0);
    lines[tail_start..]
        .iter()
        .any(|line| is_exchange_response_heading(line.trim()))
}

/// `#queue-continuation-buries-prompt`: a queue-continuation response heading
/// (`### Re: do [#id]` / `### Re: re [#id]`, any h-level) answers a queue or
/// backlog item, not a free-text user prompt. Such a heading must not mark a
/// preceding free-text exchange prompt as answered, or a queue continuation can
/// advance the boundary past an unanswered user prompt and bury it in the
/// snapshot (the JB "ignored my previous prompt" class).
pub fn is_queue_continuation_response_heading(trimmed: &str) -> bool {
    let Some(rest) = trimmed
        .strip_prefix("### Re:")
        .or_else(|| trimmed.strip_prefix("#### Re:"))
        .or_else(|| trimmed.strip_prefix("##### Re:"))
        .or_else(|| trimmed.strip_prefix("###### Re:"))
    else {
        return false;
    };
    let topic = rest.trim_start();
    // Queue-continuation topics start with the `do`/`re` directive verb plus a
    // bracketed id, e.g. "do [#6cmx]" or "re [#374n] ...".
    (topic.starts_with("do [#") || topic.starts_with("re [#")) && topic.contains(']')
}

pub fn detect_unstarted_prompt_bearing_diff(file: &Path) -> Result<Option<String>> {
    let Some(change) = first_unstarted_prompt_bearing_change(file)? else {
        return Ok(None);
    };
    let label = match change.kind {
        crate::diff::PromptBearingChangeKind::PromptTarget => "prompt_target",
        crate::diff::PromptBearingChangeKind::ContentEdit => "content_edit",
        crate::diff::PromptBearingChangeKind::RecoveryArtifact
        | crate::diff::PromptBearingChangeKind::BoundaryArtifact => return Ok(None),
    };
    let preview = change
        .text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(change.text.as_str())
        .trim();
    Ok(Some(format!("{label}: {preview}")))
}

pub fn first_unstarted_prompt_bearing_change(
    file: &Path,
) -> Result<Option<crate::diff::PromptBearingChange>> {
    // A fresh session can carry an unanswered exchange tail prompt before any
    // cycle snapshot exists. The queue path activates independently of the
    // snapshot (route queue activation re-saves the snapshot on activation), so
    // a queue write always dispatches; the exchange path relies on this diff, so
    // without a snapshot we must fall back to the committed `HEAD` blob (then to
    // an empty baseline for untracked docs) — otherwise the exchange prompt is
    // invisible and `Run Agent Doc` does nothing while the same write into the
    // queue starts a turn (#codex-exchange-prompt-no-dispatch).
    let baseline = match crate::snapshot::load(file)? {
        Some(snapshot) => snapshot,
        None => crate::git::show_head(file)?.unwrap_or_default(),
    };
    let current = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };

    let prompt_bearing_body = |content: &str| {
        let body = crate::frontmatter::parse(content)
            .map(|(_, body)| body.to_string())
            .unwrap_or_else(|_| content.to_string());
        crate::diff::strip_comments(&strip_queue_components_for_unstarted_prompt_guard(&body))
    };
    let norm = |s: &str| crate::git::normalize_committed_exchange_artifacts(s);
    let snap_norm = norm(&prompt_bearing_body(&baseline));
    let cur_norm = norm(&prompt_bearing_body(&current));
    let Some(diff_text) = crate::diff::unified_diff_from_contents(&snap_norm, &cur_norm) else {
        return Ok(None);
    };
    let changes = crate::diff::classify_prompt_bearing_changes(&diff_text);
    let mut skip_answered_response_run = false;
    for (idx, change) in changes.iter().enumerate() {
        match change.kind {
            crate::diff::PromptBearingChangeKind::RecoveryArtifact
            | crate::diff::PromptBearingChangeKind::BoundaryArtifact => continue,
            crate::diff::PromptBearingChangeKind::PromptTarget => {
                if skip_answered_response_run {
                    let preview = change
                        .text
                        .lines()
                        .find(|line| !line.trim().is_empty())
                        .unwrap_or(change.text.as_str())
                        .trim();
                    if !crate::diff::line_looks_like_fresh_prompt_after_response(preview) {
                        continue;
                    }
                }
                if crate::diff::prompt_change_is_already_answered(&change.text)
                    || crate::diff::prompt_change_is_answered_by_later_response(&changes, idx)
                    || prompt_target_is_immediately_before_existing_response(&current, &change.text)
                {
                    skip_answered_response_run = true;
                    continue;
                }
                return Ok(Some(change.clone()));
            }
            crate::diff::PromptBearingChangeKind::ContentEdit => {
                continue;
            }
        }
    }
    Ok(None)
}

pub(crate) fn strip_queue_components_for_unstarted_prompt_guard(body: &str) -> String {
    let Ok(components) = crate::component::parse(body) else {
        return body.to_string();
    };
    let mut result = body.to_string();
    for component in components.iter().rev() {
        if component.name == "queue" {
            result = component.replace_content(&result, "");
        }
    }
    result
}

pub(crate) fn prompt_target_is_immediately_before_existing_response(
    current_doc: &str,
    change_text: &str,
) -> bool {
    let target_line = change_text
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string());
    let answered_prompt_marker = target_line
        .as_deref()
        .is_some_and(|line| line.starts_with('❯'));
    let target = target_line
        .as_deref()
        .map(|line| line.trim_start_matches('❯').trim().to_string());
    let Some(target) = target else {
        return false;
    };
    if target.is_empty() {
        return false;
    }
    let body = crate::frontmatter::parse(current_doc)
        .map(|(_, body)| body.to_string())
        .unwrap_or_else(|_| current_doc.to_string());
    let Ok(components) = crate::component::parse(&body) else {
        return false;
    };
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return false;
    };
    let lines: Vec<&str> = exchange.content(&body).lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        let normalized = line.trim().trim_start_matches('❯').trim();
        if normalized != target {
            continue;
        }
        for next in lines.iter().skip(idx + 1) {
            let trimmed = next.trim();
            if trimmed.is_empty() || trimmed.starts_with("<!--") {
                continue;
            }
            if is_exchange_response_heading(trimmed) {
                return true;
            }
            if answered_prompt_marker {
                continue;
            }
            return false;
        }
    }
    false
}

pub(crate) fn prompt_only_exchange_tail(doc: &str) -> Option<String> {
    let body = crate::frontmatter::parse(doc)
        .map(|(_, body)| body.to_string())
        .unwrap_or_else(|_| doc.to_string());
    let components = crate::component::parse(&body).ok()?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")?;

    let mut in_fence: Option<&'static str> = None;
    let mut prompt_preview: Option<String> = None;
    let mut in_assistant_response = false;
    for line in exchange.content(&body).lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = match in_fence {
                Some("```") => None,
                None => Some("```"),
                other => other,
            };
            continue;
        }
        if trimmed.starts_with("~~~") {
            in_fence = match in_fence {
                Some("~~~") => None,
                None => Some("~~~"),
                other => other,
            };
            continue;
        }
        if in_fence.is_some() {
            continue;
        }
        if is_exchange_response_heading(trimmed) {
            prompt_preview = None;
            in_assistant_response = true;
            continue;
        }
        if trimmed.starts_with("<!-- agent:boundary:") || trimmed == "## User" {
            in_assistant_response = false;
            continue;
        }
        if trimmed.is_empty()
            || trimmed.starts_with("<!--")
            || trimmed == "(HEAD)"
            || crate::diff::line_looks_like_plain_response_after_prompt(trimmed)
        {
            continue;
        }
        if crate::diff::text_line_looks_like_prompt_target(trimmed) {
            if in_assistant_response && !trimmed.starts_with('❯') {
                continue;
            }
            prompt_preview.get_or_insert_with(|| {
                trimmed
                    .trim_start_matches('❯')
                    .trim()
                    .chars()
                    .take(160)
                    .collect::<String>()
            });
        }
    }
    prompt_preview
}
