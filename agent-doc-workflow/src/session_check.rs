use agent_doc_element_backlog::guard_policy::BacklogGuardOutcome;
use agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode;
use agent_doc_turn::CyclePhase;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardResult {
    None,
    Warn(Vec<String>),
    Error(String),
}

impl From<BacklogGuardOutcome> for GuardResult {
    fn from(outcome: BacklogGuardOutcome) -> Self {
        match outcome {
            BacklogGuardOutcome::Pass => Self::None,
            BacklogGuardOutcome::Warn(lines) => Self::Warn(lines),
            BacklogGuardOutcome::Interrupt(message) => Self::Error(message),
        }
    }
}

fn hash_refs(ids: &[String]) -> String {
    ids.iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn done_flags(ids: &[String]) -> String {
    ids.iter()
        .map(|id| format!("--done {id}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn pending_done_guard_result(
    file: &str,
    missing: &[String],
    mode: PendingCaptureGuardMode,
) -> GuardResult {
    let ids = hash_refs(missing);
    let hint = done_flags(missing);
    let repair = format!("agent-doc write {file} {hint} --pending-only --commit");
    let warn_line = format!(
        "[session-check] warn: response appears to complete existing pending {ids} but no matching `--done` was recorded this cycle"
    );

    match mode {
        PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!(
                "[session-check] hint: repair with `{repair}` or add `pending_done_guard: off` for this document when the item should stay open"
            ),
        ]),
        PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: repair with `{repair}` or set pending_done_guard = \"warn\" to downgrade",
            warn_line.replacen("[session-check] warn:", "[session-check] error:", 1),
        )),
        PendingCaptureGuardMode::Off => GuardResult::None,
    }
}

pub fn pending_capture_missing_targets_guard_result(missing_targets: &[String]) -> GuardResult {
    GuardResult::Error(format!(
        "[session-check] error: committed response came from a prompt that required backlog capture in {}, but those tracked-work surfaces did not change this cycle",
        missing_targets.join(", ")
    ))
}

pub fn pending_capture_inventory_shortfall_guard_result(
    expected_count: usize,
    promised_count: usize,
    targets: &[String],
) -> GuardResult {
    GuardResult::Error(format!(
        "[session-check] error: active #agent-doc-bug contract described at least {} distinct issue(s), but the committed response only enumerated {} explicit backlog item(s) for target(s) {}",
        expected_count,
        promised_count,
        targets.join(", ")
    ))
}

pub fn pending_capture_plan_reference_shortfall_guard_result(
    expected_count: usize,
    promised_count: usize,
) -> GuardResult {
    GuardResult::Error(format!(
        "[session-check] error: active #agent-doc-bug contract required at least {} explicit plan reference(s), but the committed response only cited {} existing plan path(s)",
        expected_count, promised_count,
    ))
}

pub fn pending_capture_missing_promised_ids_guard_result(
    missing_ids: &[String],
    targets: &[String],
) -> GuardResult {
    GuardResult::Error(format!(
        "[session-check] error: committed response promised new tracked item(s) {} for explicit backlog target(s) {}, but those ids are still missing after this cycle",
        missing_ids.join(", "),
        targets.join(", ")
    ))
}

pub fn pending_capture_required_no_mutations_guard_result() -> GuardResult {
    GuardResult::Error(
        "[session-check] error: committed response came from a prompt that required backlog capture, but this cycle recorded no backlog mutations and did not explicitly state that there were no actionable follow-up items"
            .to_string(),
    )
}

pub fn pending_capture_recommendations_guard_result(
    estimated_count: usize,
    mode: PendingCaptureGuardMode,
) -> GuardResult {
    let warn_line = format!(
        "[session-check] warn: response contains ~{estimated_count} recommendation-like items but no --pending-add flags were used this cycle"
    );
    let hint_line =
        "[session-check] hint: consider adding pending items for actionable follow-up work"
            .to_string();

    match mode {
        PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![warn_line, hint_line]),
        PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: re-run with --pending-add flags or set pending_capture_guard = \"warn\" to downgrade",
            warn_line.replacen("[session-check] warn:", "[session-check] error:", 1)
        )),
        PendingCaptureGuardMode::Off => GuardResult::None,
    }
}

pub fn expect_done_or_gate_guard_result(
    file: &str,
    unresolved: &[String],
    mode: PendingCaptureGuardMode,
) -> GuardResult {
    let ids = hash_refs(unresolved);
    let done_hint = done_flags(unresolved);
    let repair = format!("agent-doc write {file} {done_hint} --pending-only --commit");
    let warn_line = format!(
        "[session-check] warn: `do #id` directive resolved this cycle but tracked target {ids} is still open in agent:backlog with no `--done`, `--pending-gate`, or kept-open edit recorded"
    );

    match mode {
        PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!(
                "[session-check] hint: repair with `{repair}`, run `--pending-gate <id>` if review/external validation remains, or add `pending_done_guard: off` when the item should stay open"
            ),
        ]),
        PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: repair with `{repair}`, run `--pending-gate <id>` if review/external validation remains, or set pending_done_guard = \"warn\" to downgrade",
            warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
        )),
        PendingCaptureGuardMode::Off => GuardResult::None,
    }
}

pub fn blocked_closeout_followup_guard_result(
    file: &str,
    unresolved: &[String],
    mode: PendingCaptureGuardMode,
) -> GuardResult {
    let ids = hash_refs(unresolved);
    let edit_hint = unresolved
        .iter()
        .map(|id| format!("--backlog-edit \"{id}=<remaining next action>\""))
        .collect::<Vec<_>>()
        .join(" ");
    let add_after_hint = unresolved
        .first()
        .map(|id| format!("--backlog-add-after {id} \"<id>=<concrete next step>\""))
        .unwrap_or_default();
    let repair = format!("agent-doc write {file} {edit_hint} --pending-only --commit");
    let warn_line = format!(
        "[session-check] warn: `do #id` closeout reported blocked / still-needed work but gated tracked target {ids} out of agent:backlog with no kept-open edit, new follow-up item, or explicit no-follow-up justification — the remaining steps live only in prose"
    );

    match mode {
        PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!(
                "[session-check] hint: keep the work tracked with `{repair}`, split a new follow-up via `{add_after_hint}`, add an explicit \"no additional backlog follow-up is needed because ...\" phrase for a true review-only gate, or add `{}`",
                agent_doc_turn::closeout_signal::BLOCKED_CLOSEOUT_FOLLOWUP_GUARD_SUPPRESS_MARKER
            ),
        ]),
        PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: keep the work tracked with `{repair}`, split a new follow-up via `{add_after_hint}`, add an explicit \"no additional backlog follow-up is needed because ...\" phrase for a true review-only gate, or set pending_done_guard = \"warn\" to downgrade",
            warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
        )),
        PendingCaptureGuardMode::Off => GuardResult::None,
    }
}

pub fn partial_closeout_state_guard_result(candidate_ids: &[String]) -> GuardResult {
    if candidate_ids.is_empty() {
        return GuardResult::None;
    }

    let ids = candidate_ids
        .iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(", ");
    let edit_hint = candidate_ids
        .iter()
        .map(|id| format!("--backlog-edit \"{id}=<remaining next-phase scope>\""))
        .collect::<Vec<_>>()
        .join(" ");

    GuardResult::Warn(vec![
        format!(
            "[session-check] warn: partial `do [#id]` closeout — work shipped (committed + pushed) but the response says live deploy/sync/verification work remains, yet tracked target {ids} still carries its original full-task text in agent:backlog"
        ),
        format!(
            "[session-check] hint: narrow the backlog item + queue head to the next phase with `{edit_hint}` (or `--backlog-gate <id>` if only review/external validation remains), or add `{}` when it is already narrowed",
            agent_doc_turn::closeout_signal::PARTIAL_CLOSEOUT_GUARD_SUPPRESS_MARKER
        ),
    ])
}

pub fn gated_phase_split_guard_result(file: &str, flagged: &[String]) -> GuardResult {
    let ids = hash_refs(flagged);
    let add_after_hint = flagged
        .first()
        .map(|id| format!("--backlog-add-after {id} \"<child-id>=<one phase scope>\""))
        .unwrap_or_default();

    GuardResult::Warn(vec![
        format!(
            "[session-check] warn: kept-open tracked item {ids} enumerates multiple gated/remaining phases in its body but does not break them out into discrete child backlog IDs — the deferred phases are not independently trackable or queueable"
        ),
        format!(
            "[session-check] hint: split each gated phase into its own child id (e.g. `agent-doc write {file} {add_after_hint} --pending-only --commit`), keeping the parent as context, or add `{}` if the phases are intentionally one unit",
            agent_doc_turn::closeout_signal::GATED_PHASE_SPLIT_GUARD_SUPPRESS_MARKER
        ),
    ])
}

pub fn queue_audit_partial_completion_guard_result() -> GuardResult {
    GuardResult::Warn(vec![
        "[session-check] warn: this queue-completion audit reports the queue as not complete while also citing several completed substeps, but never classifies any row as partially complete — meaningful partial progress is collapsed into \"none complete\"".to_string(),
        format!(
            "[session-check] hint: classify each queue row as complete / partially complete / not-started, naming the completed substeps and the exact remaining condition for partial rows; recommend splitting a row with multiple gateable phases. Add `{}` if the all-or-none framing is intentional.",
            agent_doc_turn::closeout_signal::QUEUE_AUDIT_GUARD_SUPPRESS_MARKER
        ),
    ])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialStagingCloseoutGuardFinding {
    pub repo: String,
    pub committed_paths: Vec<String>,
    pub dirty_paths: Vec<String>,
    pub literals: Vec<String>,
}

pub fn partial_staging_closeout_guard_result(
    findings: &[PartialStagingCloseoutGuardFinding],
) -> GuardResult {
    if findings.is_empty() {
        return GuardResult::None;
    }

    let mut lines = Vec::new();
    for finding in findings.iter().take(3) {
        lines.push(format!(
            "[session-check] warn: possible partial staging closeout in {} — latest commit changed {}, but tracked uncommitted companion changes remain in {} with overlapping changed string literal(s): {}.",
            finding.repo,
            preview_items(&finding.committed_paths, 4),
            preview_items(&finding.dirty_paths, 4),
            preview_items(&finding.literals, 3)
        ));
    }
    if findings.len() > 3 {
        lines.push(format!(
            "[session-check] warn: {} additional partial staging candidate(s) omitted.",
            findings.len() - 3
        ));
    }
    lines.push(
        "[session-check] hint: commit the companion changes, revert them, or rerun verification against the committed tree before reporting CI-ready closeout."
            .to_string(),
    );
    GuardResult::Warn(lines)
}

fn preview_items(items: &[String], limit: usize) -> String {
    let mut preview = items
        .iter()
        .take(limit)
        .map(|item| format!("`{item}`"))
        .collect::<Vec<_>>();
    if items.len() > limit {
        preview.push(format!("...(+{})", items.len() - limit));
    }
    preview.join(", ")
}

pub fn queue_head_removal_guard_result(
    file: &str,
    lost: &[String],
    mode: PendingCaptureGuardMode,
) -> GuardResult {
    let ids = hash_refs(lost);
    let warn_line = format!(
        "[session-check] warn: runnable agent:queue head(s) {ids} were removed from the committed queue but their backlog item(s) are still open in agent:backlog, and the cycle never consumed, completed, gated, or reaped them — unrun queue work was silently dropped"
    );
    let repair = format!(
        "restore the dropped head(s) to `agent:queue` (or resolve each id with `--done`/`--pending-gate`), then re-run `agent-doc write --commit {file}`; add `<!-- no-queue-removal-guard -->` to the response if the removal was an explicit user edit"
    );

    match mode {
        PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!("[session-check] hint: {repair} (see #queue-clear-unrun-items)"),
        ]),
        PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: {repair} (see #queue-clear-unrun-items)",
            warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
        )),
        PendingCaptureGuardMode::Off => GuardResult::None,
    }
}

pub fn free_text_queue_marker_residue_result(file: &str) -> GuardResult {
    GuardResult::Error(format!(
        "[session-check] INTERRUPTED: {file} contains `<!-- no-free-text-queue-head-guard -->` plus a bare `###` heading, which is interrupted closeout evidence rather than committed response proof. Finish the response through `agent-doc finalize {file}` or `agent-doc write --commit {file}`, then run `agent-doc session-check {file}`. (see #directchatpb2)"
    ))
}

pub fn free_text_queue_completed_residue_result(file: &str, heads: &[String]) -> GuardResult {
    let heads_text = heads
        .iter()
        .map(|head| format!("{head:?}"))
        .collect::<Vec<_>>()
        .join("; ");
    GuardResult::Error(format!(
        "[session-check] INTERRUPTED: completed free-text agent:queue head(s) {heads_text} are still active in the committed queue even though exchange history contains a `Queue prompt` echo proving they were already answered — completed queue residue would re-run stale work\n[session-check] hint: remove or strike the answered head(s), then re-run `agent-doc write --commit {file}`; add `<!-- no-free-text-queue-head-guard -->` only if keeping the answered row active is intentional (see #qheadresidue)"
    ))
}

pub fn free_text_queue_head_provenance_guard_result(
    file: &str,
    unresolved: &[String],
    mode: PendingCaptureGuardMode,
) -> GuardResult {
    let heads_text = unresolved
        .iter()
        .map(|head| format!("\"{head}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let warn_line = format!(
        "[session-check] warn: free-text agent:queue head(s) {heads_text} were seen at preflight but have no committed response/echo or explicit deferral proof in the closeout — the prompt may have been silently lost"
    );
    let repair = format!(
        "either respond to the unresolved head(s) and run `agent-doc finalize {file}`, or add `<!-- no-free-text-queue-head-guard -->` if the removal was intentional"
    );
    match mode {
        PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!("[session-check] hint: {repair} (see #lr-queue-patchback-miss)"),
        ]),
        PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: {repair} (see #lr-queue-patchback-miss)",
            warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
        )),
        PendingCaptureGuardMode::Off => GuardResult::None,
    }
}

pub fn no_response_active_queue_head_result(
    file: &str,
    cycle_id: &str,
    live: &[String],
    mode: PendingCaptureGuardMode,
) -> GuardResult {
    let ids = hash_refs(live);
    let warn_line = format!(
        "[session-check] warn: cycle `{cycle_id}` committed without an assistant response body while runnable agent:queue head(s) {ids} remained queued and open in agent:backlog; this was a no-response repair/reap-only closeout, not a completed queue turn"
    );
    let repair = format!(
        "run `agent-doc {file}` from the owning session so the queued head is answered, or resolve each id through `agent-doc write --commit {file}` with `--done`, `--pending-gate`, or `--pending-edit` proof before closing"
    );

    match mode {
        PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!("[session-check] hint: {repair} (see #nochange-after-stall-breadth)"),
        ]),
        PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: {repair} (see #nochange-after-stall-breadth)",
            warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
        )),
        PendingCaptureGuardMode::Off => GuardResult::None,
    }
}

pub fn reaped_queue_head_without_response_result(
    file: &str,
    cycle_id: &str,
    lost: &[String],
    mode: PendingCaptureGuardMode,
) -> GuardResult {
    let ids = hash_refs(lost);
    let warn_line = format!(
        "[session-check] warn: cycle `{cycle_id}` reaped `do` queue-directive head(s) {ids} into agent:done without an assistant response landing in agent:exchange (no response body this cycle and no `### Re:` for the id in the exchange or a HEAD compact archive); the response record was silently lost"
    );
    let repair = format!(
        "recover the lost response by re-running `agent-doc {file}` so the directive id is answered, or restore the missing `### Re:` block through `agent-doc write --commit {file}` before closing"
    );

    match mode {
        PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!("[session-check] hint: {repair} (see #compact-reap-no-response-record)"),
        ]),
        PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: {repair} (see #compact-reap-no-response-record)",
            warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
        )),
        PendingCaptureGuardMode::Off => GuardResult::None,
    }
}

pub fn dropped_queue_prompt_guard_result(file: &str, still_missing: &[String]) -> GuardResult {
    GuardResult::Error(format!(
        "[session-check] INTERRUPTED: user-authored agent:queue edit(s) were dropped during an IPC content_ours merge and are missing from the visible document without being consumed: {}. Convergence overwrote a newer visible queue; re-add them to `agent:queue` and re-run `agent-doc finalize {file}` / `agent-doc write --commit {file}` so the queued work is preserved (see #queue-user-edit-overwrite).",
        still_missing.join("; "),
    ))
}

pub fn queue_response_contamination_guard_result(contaminated: &[String]) -> GuardResult {
    GuardResult::Error(format!(
        "[session-check] INTERRUPTED: agent:queue contains assistant response prose copied from a `### Re:` body, not a user prompt or `do [#id]` directive: {}. Remove the contaminating line(s) from `agent:queue` (only user prompts, `do [#id]`, `preset`/`dispatch`, or backlog-derived entries are valid queue sources) and re-run finalize (see #jb-run-agent-doc-response-queue-contamination).",
        contaminated
            .iter()
            .map(|text| format!("{text:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// Free-text queue prompt candidates that appear copied from assistant response
/// prose in the same exchange body.
pub fn queue_response_contamination_candidates(
    queue_body: &str,
    exchange_body: &str,
) -> Vec<String> {
    let Ok(entries) = agent_doc_queue::document_queue::parse(queue_body) else {
        return Vec::new();
    };
    let response_text = agent_doc_turn::closeout_signal::assistant_response_text(exchange_body);
    if response_text.trim().is_empty() {
        return Vec::new();
    }

    let mut contaminated = Vec::new();
    for prompt in agent_doc_queue::document_queue::prompts(&entries) {
        let text = prompt.text.trim();
        if text.is_empty() || agent_doc_queue::queue_command::is_queue_directive_prompt(text) {
            continue;
        }
        if agent_doc_queue::queue_command::mentions_slash_command_reference(text) {
            continue;
        }
        let normalized = agent_doc_turn::closeout_signal::normalized_prompt_for_match(text);
        if normalized.chars().count() < 20 {
            continue;
        }
        let needle: String = normalized.chars().take(40).collect();
        if response_text.contains(&needle) {
            contaminated.push(text.chars().take(80).collect::<String>());
        }
    }

    contaminated
}

pub fn dropped_exchange_prompt_guard_result(file: &str, still_missing: &[String]) -> GuardResult {
    GuardResult::Error(format!(
        "[session-check] INTERRUPTED: user-authored exchange prompt(s) were dropped during an IPC content_ours merge and are missing from the committed document: {}. The cycle committed `content_ours` without these prompt-bearing line(s); re-add them to `agent:exchange` and re-run `agent-doc finalize {file}` / `agent-doc write --commit {file}` so they are answered (see #exchange-prompt-dropped-on-merge).",
        still_missing.join("; "),
    ))
}

pub fn completed_pending_reap_guard_message(refs: &str) -> String {
    format!(
        "[session-check] INTERRUPTED: document still contains completed tracked item(s) after closeout: {refs}. Re-run preflight/repair so the reap is persisted through the snapshot + commit boundary"
    )
}

pub fn snapshot_committed_guard_message(
    snapshot_len: usize,
    head_len: usize,
    side_effects: &str,
    recovery_hint: &str,
) -> String {
    format!(
        "[session-check] INTERRUPTED: cycle state is committed but the snapshot does not match HEAD in the owning repo (snapshot_len={snapshot_len}, head_len={head_len}). The response patchback is visible but was never committed{side_effects} {recovery_hint}"
    )
}

pub fn committed_without_response_body_guard_message(
    cycle_id: &str,
    last_event: &str,
    side_effects: &str,
    recovery_hint: &str,
) -> String {
    format!(
        "[session-check] INTERRUPTED: cycle committed binary-owned work this turn but no assistant `### Re:` response body is present in `agent:exchange` (cycle `{cycle_id}`, last_event `{last_event}`). The close-out response was never written into `agent:exchange`{side_effects} (#codex-queue-drain-no-response-body). {recovery_hint}"
    )
}

pub fn prompt_only_exchange_tail_guard_message(prompt: &str, file: &str) -> String {
    format!(
        "[session-check] INTERRUPTED: live exchange ends with unresolved prompt-only closeout tail and no assistant response: {prompt}. Finish the turn through `agent-doc finalize {file}` or recover the missing response with `agent-doc write --commit {file}` before reporting closeout success."
    )
}

pub fn prompt_only_exchange_tail_guard(content: &str, file: &str) -> Option<String> {
    let prompt = agent_doc_turn::exchange_tail::prompt_only_exchange_tail(content)?;
    Some(prompt_only_exchange_tail_guard_message(&prompt, file))
}

pub fn likely_direct_response_patchback_message(
    marker: &str,
    side_effects: &str,
    recovery_hint: &str,
) -> String {
    format!(
        "[session-check] INTERRUPTED: found likely direct response patchback without agent-doc cycle: {marker}{side_effects} {recovery_hint}"
    )
}

pub fn committed_head_response_missing_message(
    file: &str,
    cycle_id: &str,
    phase: CyclePhase,
    last_event: &str,
    heading: &str,
) -> String {
    format!(
        "[session-check] INTERRUPTED: cycle `{cycle_id}` is `{}` ({last_event}), but the current visible document is missing the latest committed HEAD response `{heading}`. Preserve any current operator edits, then rerun `agent-doc write --commit {file}` so realtime closeout can merge the committed response back into the visible document.",
        phase.as_str(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockedCloseoutMessage<'a> {
    pub file: &'a str,
    pub kind: &'a str,
    pub cycle_id: &'a str,
    pub phase: CyclePhase,
    pub last_event: &'a str,
    pub source: &'a str,
    pub reason: &'a str,
    pub patch_id: Option<&'a str>,
    pub recovery: Option<&'a str>,
    pub detail: Option<&'a str>,
    pub recovery_command: Option<&'a str>,
    pub editor_authority_note: &'a str,
}

pub fn blocked_closeout_message(input: BlockedCloseoutMessage<'_>) -> String {
    let patch = input
        .patch_id
        .map(|id| format!(" patch_id={id}"))
        .unwrap_or_default();
    let recovery = input
        .recovery
        .map(|value| format!(" recovery={value}"))
        .unwrap_or_default();
    let detail = input
        .detail
        .map(|value| format!(" detail={value}"))
        .unwrap_or_default();
    if input.kind == "editor_convergence_required" {
        return format!(
            "[session-check] INTERRUPTED: closeout blocked by `{}` for cycle `{}` (phase={} last_event={} source={} reason={}{}{}{}).{} The response/patch is durably retained under its original capture identity. The binary-owned supervisor will resume reconcile, editor delivery, and exact-once commit after the editor replica converges; `session-check` is status-only. Do not rerun `finalize`, run `write --commit`, kill the controller, or use `--force-disk` for this capture.",
            input.kind,
            input.cycle_id,
            input.phase.as_str(),
            input.last_event,
            input.source,
            input.reason,
            patch,
            recovery,
            detail,
            input.editor_authority_note,
        );
    }
    let retry = input
        .recovery_command
        .map(str::to_string)
        .unwrap_or_else(|| format!("agent-doc write --commit {}", input.file));
    format!(
        "[session-check] INTERRUPTED: closeout blocked by `{}` for cycle `{}` (phase={} last_event={} source={} reason={}{}{}{}).{} The response/patch is retained for editor retry; save or resolve the live editor buffer, then run `{}`. Use `{} --force-disk` only after an explicit operator decision to override the live-editor safety guard.",
        input.kind,
        input.cycle_id,
        input.phase.as_str(),
        input.last_event,
        input.source,
        input.reason,
        patch,
        recovery,
        detail,
        input.editor_authority_note,
        retry,
        retry,
    )
}

pub fn open_cycle_detail(phase: CyclePhase) -> &'static str {
    match phase {
        CyclePhase::PreflightStarted => "cycle started but no write/commit followed",
        CyclePhase::ResponseCaptured => "response was captured but no write/commit followed",
        CyclePhase::WriteApplied => "response write landed but no terminal commit followed",
        CyclePhase::Committed => "no terminal commit followed",
        CyclePhase::Abandoned => "cycle was abandoned",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenCycleMessage<'a> {
    pub file: &'a str,
    pub cycle_id: &'a str,
    pub phase: CyclePhase,
    pub last_event: &'a str,
    pub ipc_hint: &'a str,
}

pub fn open_cycle_message(input: OpenCycleMessage<'_>) -> String {
    if input.last_event.starts_with("direct_invocation_timeout")
        || input
            .last_event
            .starts_with("recursive_direct_invocation_blocked")
    {
        return format!(
            "[session-check] INTERRUPTED: cycle `{}` is still `{}` ({}) — direct invocation did not reach response capture. If the owning pane is now idle but the document still reports busy, reconcile it without killing the pane via `agent-doc session status {}` (or `agent-doc session clear {}`). Otherwise retry from outside the managed pane, restart the owner with `agent-doc start {}`, or abandon the stale cycle only after confirming no response exists.{}",
            input.cycle_id,
            input.phase.as_str(),
            input.last_event,
            input.file,
            input.file,
            input.file,
            input.ipc_hint
        );
    }
    format!(
        "[session-check] INTERRUPTED: cycle `{}` is still `{}` ({}) — {}.{}",
        input.cycle_id,
        input.phase.as_str(),
        input.last_event,
        open_cycle_detail(input.phase),
        input.ipc_hint
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenCycleManualPatchbackMessage<'a> {
    pub file: &'a str,
    pub cycle_id: &'a str,
    pub phase: CyclePhase,
    pub last_event: &'a str,
    pub marker: &'a str,
}

pub fn open_cycle_manual_patchback_message(input: OpenCycleManualPatchbackMessage<'_>) -> String {
    format!(
        "[session-check] INTERRUPTED: cycle `{}` is still `{}` ({}) — found visible response patchback {} that is still outside the commit boundary. This looks like a manual repair that stopped before commit; finish it with `agent-doc write --commit {}` if you still have the response body, or commit the repaired document manually once the response is correct.",
        input.cycle_id,
        input.phase.as_str(),
        input.last_event,
        input.marker,
        input.file
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_response_active_queue_head_result_formats_modes() {
        let live = vec!["alpha".to_string(), "bravo".to_string()];

        let warn = no_response_active_queue_head_result(
            "task.md",
            "cycle-1",
            &live,
            PendingCaptureGuardMode::Warn,
        );
        let GuardResult::Warn(lines) = warn else {
            panic!("expected warning result");
        };
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("cycle `cycle-1` committed without an assistant response body"));
        assert!(lines[0].contains("#alpha, #bravo"));
        assert!(lines[1].contains("agent-doc write --commit task.md"));
        assert!(lines[1].contains("#nochange-after-stall-breadth"));

        let strict = no_response_active_queue_head_result(
            "task.md",
            "cycle-1",
            &live,
            PendingCaptureGuardMode::Strict,
        );
        let GuardResult::Error(message) = strict else {
            panic!("expected strict error result");
        };
        assert!(message.starts_with("[session-check] INTERRUPTED: cycle `cycle-1` committed"));
        assert!(message.contains("#nochange-after-stall-breadth"));

        assert_eq!(
            no_response_active_queue_head_result(
                "task.md",
                "cycle-1",
                &live,
                PendingCaptureGuardMode::Off,
            ),
            GuardResult::None
        );
    }

    #[test]
    fn pending_capture_result_builders_format_error_cases() {
        let targets = vec!["tasks/a.md".to_string(), "tasks/b.md".to_string()];
        let missing_ids = vec!["new1".to_string(), "new2".to_string()];

        assert!(matches!(
            pending_capture_missing_targets_guard_result(&targets),
            GuardResult::Error(message)
                if message.contains("required backlog capture in tasks/a.md, tasks/b.md")
        ));
        assert!(matches!(
            pending_capture_inventory_shortfall_guard_result(3, 1, &targets),
            GuardResult::Error(message)
                if message.contains("at least 3 distinct issue(s)")
                    && message.contains("1 explicit backlog item(s)")
                    && message.contains("tasks/a.md, tasks/b.md")
        ));
        assert!(matches!(
            pending_capture_plan_reference_shortfall_guard_result(2, 0),
            GuardResult::Error(message)
                if message.contains("at least 2 explicit plan reference(s)")
                    && message.contains("cited 0 existing plan path(s)")
        ));
        assert!(matches!(
            pending_capture_missing_promised_ids_guard_result(&missing_ids, &targets),
            GuardResult::Error(message)
                if message.contains("new1, new2") && message.contains("tasks/a.md, tasks/b.md")
        ));
        assert!(matches!(
            pending_capture_required_no_mutations_guard_result(),
            GuardResult::Error(message)
                if message.contains("recorded no backlog mutations")
        ));
    }

    #[test]
    fn pending_capture_recommendations_result_formats_modes() {
        let warn = pending_capture_recommendations_guard_result(4, PendingCaptureGuardMode::Warn);
        let GuardResult::Warn(lines) = warn else {
            panic!("expected warning result");
        };
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("~4 recommendation-like items"));
        assert!(lines[1].contains("consider adding pending items"));

        let strict =
            pending_capture_recommendations_guard_result(2, PendingCaptureGuardMode::Strict);
        assert!(matches!(
            strict,
            GuardResult::Error(message)
                if message.starts_with("[session-check] error: response contains ~2")
                    && message.contains("set pending_capture_guard = \"warn\"")
        ));

        assert_eq!(
            pending_capture_recommendations_guard_result(2, PendingCaptureGuardMode::Off),
            GuardResult::None
        );
    }

    #[test]
    fn reaped_queue_head_without_response_result_formats_modes() {
        let lost = vec!["charlie".to_string()];

        let warn = reaped_queue_head_without_response_result(
            "task.md",
            "cycle-2",
            &lost,
            PendingCaptureGuardMode::Warn,
        );
        let GuardResult::Warn(lines) = warn else {
            panic!("expected warning result");
        };
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("cycle `cycle-2` reaped `do` queue-directive head(s) #charlie"));
        assert!(lines[0].contains("the response record was silently lost"));
        assert!(lines[1].contains("agent-doc write --commit task.md"));
        assert!(lines[1].contains("#compact-reap-no-response-record"));

        let strict = reaped_queue_head_without_response_result(
            "task.md",
            "cycle-2",
            &lost,
            PendingCaptureGuardMode::Strict,
        );
        let GuardResult::Error(message) = strict else {
            panic!("expected strict error result");
        };
        assert!(message.starts_with("[session-check] INTERRUPTED: cycle `cycle-2` reaped `do`"));
        assert!(message.contains("#compact-reap-no-response-record"));

        assert_eq!(
            reaped_queue_head_without_response_result(
                "task.md",
                "cycle-2",
                &lost,
                PendingCaptureGuardMode::Off,
            ),
            GuardResult::None
        );
    }

    #[test]
    fn editor_convergence_message_is_binary_owned_and_status_only() {
        let message = blocked_closeout_message(BlockedCloseoutMessage {
            file: "task.md",
            kind: "editor_convergence_required",
            cycle_id: "cycle-1",
            phase: CyclePhase::WriteApplied,
            last_event: "write_applied",
            source: "write",
            reason: "live buffer differs",
            patch_id: Some("patch-7"),
            recovery: Some("retry"),
            detail: Some("needs save"),
            recovery_command: None,
            editor_authority_note: " Live editor `jb` lacks capability.",
        });

        assert!(message.contains("closeout blocked by `editor_convergence_required`"));
        assert!(message.contains("phase=write_applied"));
        assert!(message.contains("patch_id=patch-7 recovery=retry detail=needs save"));
        assert!(message.contains("Live editor `jb` lacks capability."));
        assert!(message.contains("binary-owned supervisor"));
        assert!(message.contains("`session-check` is status-only"));
        assert!(message.contains("Do not rerun `finalize`"));
        assert!(!message.contains("then run `agent-doc write --commit"));
    }

    #[test]
    fn queue_response_contamination_candidates_flags_response_prose() {
        let prose = "Yes. I drove the already-authenticated Google Ads browser session with chromium-bridge to demote the campaign.";
        let queue = format!("- do [#nbsearch]\n- {prose}\n");
        let exchange = format!("### Re: #gads106demote\n\n{prose}\n");

        assert_eq!(
            queue_response_contamination_candidates(&queue, &exchange),
            vec![
                "Yes. I drove the already-authenticated Google Ads browser session with chromium-"
                    .to_string()
            ]
        );
    }

    #[test]
    fn queue_response_contamination_candidates_skips_directives_short_text_and_slash_references() {
        let exchange = concat!(
            "### Re: clear opt-in\n\n",
            "JB Run Agent Doc /clear opt-in should pre-emptively run /clear at the configured threshold.\n",
            "Short prompt.\n",
            "Done with the implementation.\n",
        );
        let queue = concat!(
            "- do [#nbsearch]\n",
            "- short prompt\n",
            "- JB Run Agent Doc /clear opt-in should pre-emptively run /clear when the context threshold is exceeded\n",
        );

        assert!(queue_response_contamination_candidates(queue, exchange).is_empty());
    }

    #[test]
    fn queue_response_contamination_candidates_ignores_blockquoted_prompt_echo() {
        let head = "The backlog has not been updating with the queue progress. Some queue items remain uncommitted over several runs.";
        let exchange = format!(
            concat!(
                "### Re: Backlog freshness\n\n",
                "> **Queue prompt:**\n>\n> {head}\n\n",
                "Diagnosed the freshness symptom.\n",
            ),
            head = head
        );
        let queue = format!("- {head}\n");

        assert!(queue_response_contamination_candidates(&queue, &exchange).is_empty());
    }

    #[test]
    fn partial_closeout_state_guard_result_formats_candidates() {
        let candidates = vec!["nstep2".to_string(), "deploy7".to_string()];

        let GuardResult::Warn(lines) = partial_closeout_state_guard_result(&candidates) else {
            panic!("expected warning result");
        };

        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("#nstep2, #deploy7"));
        assert!(lines[0].contains("partial `do [#id]` closeout"));
        assert!(lines[1].contains(
            "--backlog-edit \"nstep2=<remaining next-phase scope>\" --backlog-edit \"deploy7=<remaining next-phase scope>\""
        ));
        assert!(
            lines[1]
                .contains(agent_doc_turn::closeout_signal::PARTIAL_CLOSEOUT_GUARD_SUPPRESS_MARKER)
        );
        assert_eq!(partial_closeout_state_guard_result(&[]), GuardResult::None);
    }

    #[test]
    fn partial_staging_closeout_guard_result_formats_previews_and_omissions() {
        let first = PartialStagingCloseoutGuardFinding {
            repo: "/repo/one".to_string(),
            committed_paths: vec![
                "src/render.rs".to_string(),
                "src/lib.rs".to_string(),
                "src/view.rs".to_string(),
                "src/model.rs".to_string(),
                "src/extra.rs".to_string(),
            ],
            dirty_paths: vec!["tests/render_test.rs".to_string()],
            literals: vec![
                "new queue output".to_string(),
                "alt output".to_string(),
                "third output".to_string(),
                "fourth output".to_string(),
            ],
        };
        let extra = PartialStagingCloseoutGuardFinding {
            repo: "/repo/extra".to_string(),
            committed_paths: vec!["src/a.rs".to_string()],
            dirty_paths: vec!["tests/a_test.rs".to_string()],
            literals: vec!["same literal".to_string()],
        };
        let findings = vec![first, extra.clone(), extra.clone(), extra];

        let GuardResult::Warn(lines) = partial_staging_closeout_guard_result(&findings) else {
            panic!("expected warning result");
        };

        assert_eq!(lines.len(), 5);
        assert!(lines[0].contains("possible partial staging closeout in /repo/one"));
        assert!(
            lines[0]
                .contains("`src/render.rs`, `src/lib.rs`, `src/view.rs`, `src/model.rs`, ...(+1)")
        );
        assert!(lines[0].contains("`tests/render_test.rs`"));
        assert!(lines[0].contains("`new queue output`, `alt output`, `third output`, ...(+1)"));
        assert!(
            lines[3].contains(
                "[session-check] warn: 1 additional partial staging candidate(s) omitted."
            )
        );
        assert!(lines[4].contains("commit the companion changes"));
        assert_eq!(
            partial_staging_closeout_guard_result(&[]),
            GuardResult::None
        );
    }

    #[test]
    fn blocked_closeout_message_honors_recovery_command() {
        let message = blocked_closeout_message(BlockedCloseoutMessage {
            file: "task.md",
            kind: "merge_blocked",
            cycle_id: "cycle-2",
            phase: CyclePhase::ResponseCaptured,
            last_event: "capture",
            source: "merge",
            reason: "conflict",
            patch_id: None,
            recovery: None,
            detail: None,
            recovery_command: Some("agent-doc write --commit task.md --retry-patch patch-9"),
            editor_authority_note: "",
        });

        assert!(message.contains("run `agent-doc write --commit task.md --retry-patch patch-9`"));
        assert!(
            message.contains(
                "Use `agent-doc write --commit task.md --retry-patch patch-9 --force-disk`"
            )
        );
        assert!(!message.contains("patch_id="));
    }

    #[test]
    fn open_cycle_message_formats_direct_invocation_recovery() {
        let message = open_cycle_message(OpenCycleMessage {
            file: "doc.md",
            cycle_id: "cycle-3",
            phase: CyclePhase::PreflightStarted,
            last_event: "direct_invocation_timeout after 30s",
            ipc_hint: " ipc proof pending",
        });

        assert!(message.contains("direct invocation did not reach response capture"));
        assert!(message.contains("agent-doc session status doc.md"));
        assert!(message.contains("agent-doc start doc.md"));
        assert!(message.ends_with(" ipc proof pending"));
    }

    #[test]
    fn open_cycle_message_formats_phase_detail() {
        let message = open_cycle_message(OpenCycleMessage {
            file: "doc.md",
            cycle_id: "cycle-4",
            phase: CyclePhase::ResponseCaptured,
            last_event: "capture_response",
            ipc_hint: "",
        });

        assert!(message.contains("response was captured but no write/commit followed"));
    }

    #[test]
    fn open_cycle_manual_patchback_message_formats_recovery() {
        let message = open_cycle_manual_patchback_message(OpenCycleManualPatchbackMessage {
            file: "doc.md",
            cycle_id: "cycle-5",
            phase: CyclePhase::PreflightStarted,
            last_event: "preflight_diff_start",
            marker: "### Re: answer",
        });

        assert!(message.contains("found visible response patchback ### Re: answer"));
        assert!(message.contains("agent-doc write --commit doc.md"));
    }

    #[test]
    fn extracted_session_check_messages_format_context() {
        let prompt_message = prompt_only_exchange_tail_guard_message("please continue", "doc.md");
        assert!(prompt_message.contains("unresolved prompt-only closeout tail"));
        assert!(prompt_message.contains("agent-doc finalize doc.md"));

        let patchback_message = likely_direct_response_patchback_message(
            "### Re: answer",
            "; tracked side-effect edits: src/lib.rs",
            "Recovery [retry]: agent-doc write --commit doc.md.",
        );
        assert!(patchback_message.contains("### Re: answer; tracked side-effect edits"));
        assert!(patchback_message.contains("Recovery [retry]"));

        let missing_message = committed_head_response_missing_message(
            "doc.md",
            "cycle-1",
            CyclePhase::Committed,
            "commit_success",
            "### Re: latest",
        );
        assert!(missing_message.contains("cycle `cycle-1` is `committed`"));
        assert!(missing_message.contains("missing the latest committed HEAD response"));
        assert!(missing_message.contains("agent-doc write --commit doc.md"));
    }

    #[test]
    fn prompt_only_exchange_tail_guard_detects_unanswered_tail() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "do [#next]\n",
            "<!-- /agent:exchange -->\n",
        );

        let message = prompt_only_exchange_tail_guard(content, "doc.md")
            .expect("prompt-only tail should interrupt session-check");

        assert!(message.contains("do [#next]"), "{message}");
        assert!(message.contains("agent-doc finalize doc.md"), "{message}");
    }
}
