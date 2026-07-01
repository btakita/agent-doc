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
    fn blocked_closeout_message_uses_default_retry_and_optional_fields() {
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
        assert!(message.contains("run `agent-doc write --commit task.md`"));
        assert!(message.contains("Use `agent-doc write --commit task.md --force-disk`"));
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
}
