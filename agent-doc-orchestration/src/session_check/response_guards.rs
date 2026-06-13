//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn normalized_prompt_for_match(line: &str) -> String {
    line.trim().trim_start_matches('❯').trim().to_string()
}

/// True when `doc`'s `agent:exchange` component contains a line matching the
/// given prompt (normalized: leading `❯` and whitespace stripped). Used to
/// decide whether a recorded dropped prompt has been resolved (reached the
/// committed document) so the guard can clear and stop firing.
pub(crate) fn exchange_contains_prompt_line(doc: &str, prompt: &str) -> bool {
    let needle = normalized_prompt_for_match(prompt);
    if needle.is_empty() {
        return true;
    }
    let Ok(components) = crate::component::parse(doc) else {
        return false;
    };
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return false;
    };
    exchange
        .content(doc)
        .lines()
        .any(|line| normalized_prompt_for_match(line) == needle)
}

/// True when the committed `doc`'s `agent:queue` still contains the given queue
/// prompt line (normalized). Used to decide whether a dropped user queue edit
/// reached HEAD (preserved) so the guard can clear.
pub(crate) fn normalized_queue_line_for_match(line: &str) -> String {
    let trimmed = line.trim();
    let trimmed = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
        .unwrap_or(trimmed)
        .trim();
    // #queue-user-edit-overwrite false positive: a consumed queue head is
    // struck in place (`~~text~~`, legacy `~text~`) and queue maintenance may
    // pin lines (`:round_pushpin:` / `:pushpin:`), so strip both before
    // identity matching — a struck or re-pinned line visibly reached the
    // document and was not silently lost.
    let unstruck = trimmed
        .strip_prefix("~~")
        .and_then(|s| s.strip_suffix("~~"))
        .or_else(|| trimmed.strip_prefix('~').and_then(|s| s.strip_suffix('~')))
        .unwrap_or(trimmed);
    let unpinned = crate::queue::strip_priority_markers(unstruck);
    normalized_prompt_for_match(&unpinned)
}

pub(crate) fn queue_contains_prompt_line(doc: &str, prompt: &str) -> bool {
    let needle = normalized_queue_line_for_match(prompt);
    if needle.is_empty() {
        return true;
    }
    let Ok(components) = crate::component::parse(doc) else {
        return false;
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return false;
    };
    queue
        .content(doc)
        .lines()
        .any(|line| normalized_queue_line_for_match(line) == needle)
}

/// `#queue-user-edit-overwrite`: fail closed when this cycle recorded a
/// user-authored `agent:queue` edit dropped during a `content_ours` IPC adoption
/// and that queue line is still absent from the committed `HEAD` — unless the
/// current response legitimately consumed it (its `do [#id]` id reached a
/// lifecycle outcome this cycle). A preserved queue line (reached HEAD's queue
/// or exchange) or a consumed head clears the marker; a silently-deleted user
/// queue edit fails closed.
pub(crate) fn check_dropped_queue_prompt_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.dropped_queue_prompts.is_empty() {
        return Ok(GuardResult::None);
    }
    // Unlike the exchange guard, a user queue edit is SUPPOSED to stay out of
    // HEAD: `content_ours` adoption preserves it on disk so it re-surfaces as a
    // next-cycle diff. The loss case is the edit vanishing from the visible
    // document, so check the current file (and HEAD as a committed fallback).
    // Phase 6 (#lr-content-6): cached document content via `DocContentCell`.
    let visible = rc.doc_content();
    let head_content = rc.head_content();
    let head = head_content
        .as_deref()
        .map(String::as_str)
        .unwrap_or_default();
    let resolved_ids = crate::cycle_state::resolved_pending_ids(file)?;
    let still_missing: Vec<String> = state
        .dropped_queue_prompts
        .iter()
        .filter(|prompt| {
            // Preserved in the visible/HEAD queue, or answered in the
            // visible/HEAD exchange → kept, not lost.
            if queue_contains_prompt_line(&visible, prompt)
                || queue_contains_prompt_line(head, prompt)
                || exchange_contains_prompt_line(&visible, prompt)
                || exchange_contains_prompt_line(head, prompt)
            {
                return false;
            }
            // Legitimately consumed this cycle: the queued `do [#id]` id reached
            // a done/gate/reap outcome, so deleting the queue line is correct.
            let consumed = do_directive_target_ids(std::slice::from_ref(prompt))
                .into_iter()
                .any(|id| resolved_ids.contains(&id));
            !consumed
        })
        .cloned()
        .collect();
    if still_missing.is_empty() {
        crate::cycle_state::clear_dropped_queue_prompts(file)?;
        return Ok(GuardResult::None);
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "dropped_queue_prompt_guard_failed file={} count={}",
            file.display(),
            still_missing.len()
        ),
    );
    Ok(GuardResult::Error(format!(
        "[session-check] INTERRUPTED: user-authored agent:queue edit(s) were dropped during an IPC content_ours merge and are missing from the visible document without being consumed: {}. Convergence overwrote a newer visible queue; re-add them to `agent:queue` and re-run `agent-doc finalize {}` / `agent-doc write --commit {}` so the queued work is preserved (see #queue-user-edit-overwrite).",
        still_missing.join("; "),
        file.display(),
        file.display()
    )))
}

/// Collect the assistant `### Re:` response prose from an exchange component
/// body (heading + boundary + comment + blockquote lines excluded). Used to
/// detect queue lines that were copied out of a response body.
///
/// Blockquote lines (`> …`) are EXCLUDED: every `### Re:` response echoes the
/// prompt it is answering as a `> **Queue prompt:** … > <verbatim head text>`
/// quote. Including those echoes made the contamination guard
/// (`check_queue_response_contamination_guard`) false-positive on any
/// still-live free-text queue head whose text a response quoted — the response
/// legitimately quotes the head it answered, and an earlier response that
/// quoted a near-identical prompt would flag an unrelated live queue item too.
/// The guard targets assistant ANSWER prose copied into the queue, not the
/// prompt-echo, so blockquoted quotes are not response prose for this purpose.
/// (#jb-run-agent-doc-response-queue-contamination — blockquote-echo false positive)
pub(crate) fn assistant_response_text(exchange_body: &str) -> String {
    let mut in_response = false;
    let mut out = String::new();
    for line in exchange_body.lines() {
        let trimmed = line.trim();
        if is_exchange_response_heading(trimmed) {
            in_response = true;
            continue;
        }
        if trimmed.starts_with("<!-- agent:boundary") {
            continue;
        }
        if in_response
            && !trimmed.is_empty()
            && !trimmed.starts_with("<!--")
            && !trimmed.starts_with('>')
        {
            out.push_str(trimmed);
            out.push('\n');
        }
    }
    out
}

/// True when a queue prompt's text is a recognized directive / id-bearing prompt
/// (a legitimate queue entry shape) rather than free-text prose.
pub(crate) fn is_queue_directive_prompt(text: &str) -> bool {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    lower.starts_with("do ")
        || lower.starts_with("preset ")
        || lower.starts_with("dispatch ")
        || lower.starts_with("run ")
        || t.starts_with('#')
        || t.contains("[#")
}

/// True when a queue prompt references a slash command (e.g. `/agent-doc`,
/// `/clear`, `/compact`, `/loop`) at a token boundary.
///
/// `#queue-contamination-guard-false-positive`: such a prompt is a
/// user-authored instruction, not assistant answer prose copied into the queue.
/// The contamination guard targets declarative answer prose ("Yes. I drove
/// ..."), which never leads with slash-command instructions, so a legit user
/// prompt that mentions `/agent-doc`/`/clear` must not be flagged just because
/// it shares a verbatim 40-char run with a response that discussed the same
/// commands. (A leading `/` is also a unix-path shape, which is likewise user
/// content, not copied answer prose — so this skip stays conservative.)
pub(crate) fn mentions_slash_command(text: &str) -> bool {
    let bytes = text.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'/' {
            continue;
        }
        // The slash must begin a token (start of string or after a non-word char)
        // so `src/agent-doc` (a path segment after a word char) is not matched.
        let at_token_start = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        if !at_token_start {
            continue;
        }
        // Require at least two command-name chars after the slash so a bare
        // separator `/` is not treated as a command reference.
        let cmd_len = text[i + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .count();
        if cmd_len >= 2 && text[i + 1..].starts_with(|c: char| c.is_ascii_alphabetic()) {
            return true;
        }
    }
    false
}

/// `#jb-run-agent-doc-response-queue-contamination`: `Run Agent Doc` / queue
/// synthesis must never enqueue assistant response prose. The live repro added
/// `- Yes. I drove the already-authenticated Google Ads browser session ...`
/// (copied from a `### Re:` body) to `agent:queue auto`. Detect a free-text
/// queue prompt whose text appears inside an assistant response body and fail
/// closed naming the contaminating candidate.
pub(crate) fn check_queue_response_contamination_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached content + parsed components.
    let content = rc.doc_content();
    let components = rc.components();
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return Ok(GuardResult::None);
    };
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return Ok(GuardResult::None);
    };

    let queue_body = &content[queue.open_end..queue.close_start];
    let Ok(entries) = crate::queue::parse(queue_body) else {
        return Ok(GuardResult::None);
    };
    let response_text = assistant_response_text(exchange.content(&content));
    if response_text.trim().is_empty() {
        return Ok(GuardResult::None);
    }

    let mut contaminated: Vec<String> = Vec::new();
    for prompt in crate::queue::prompts(&entries) {
        let text = prompt.text.trim();
        if text.is_empty() || is_queue_directive_prompt(text) {
            continue;
        }
        // #queue-contamination-guard-false-positive: a queue prompt that
        // references a slash command (/agent-doc, /clear, /compact, ...) is a
        // user instruction, not copied answer prose — skip it.
        if mentions_slash_command(text) {
            continue;
        }
        // Only treat substantial prose as a contamination candidate; short
        // free-text prompts are legitimate (`#free-text-queue-head-consume`).
        let normalized = normalized_prompt_for_match(text);
        if normalized.chars().count() < 20 {
            continue;
        }
        let needle: String = normalized.chars().take(40).collect();
        if response_text.contains(&needle) {
            contaminated.push(text.chars().take(80).collect::<String>());
        }
    }

    if contaminated.is_empty() {
        return Ok(GuardResult::None);
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "queue_response_contamination_guard_failed file={} count={}",
            file.display(),
            contaminated.len()
        ),
    );
    Ok(GuardResult::Error(format!(
        "[session-check] INTERRUPTED: agent:queue contains assistant response prose copied from a `### Re:` body, not a user prompt or `do [#id]` directive: {}. Remove the contaminating line(s) from `agent:queue` (only user prompts, `do [#id]`, `preset`/`dispatch`, or backlog-derived entries are valid queue sources) and re-run finalize (see #jb-run-agent-doc-response-queue-contamination).",
        contaminated
            .iter()
            .map(|t| format!("{:?}", t))
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

/// `#exchange-prompt-dropped-on-merge`: fail closed when this cycle recorded a
/// user-authored exchange prompt dropped during a `content_ours` IPC adoption
/// and that prompt is still absent from the committed `HEAD`. The evidence is
/// persisted at adoption time, so this guard catches the silent-loss class even
/// when the editor overwrote the disk prompt via IPC buffer convergence before
/// the post-commit disk diff could observe it.
pub(crate) fn check_dropped_exchange_prompt_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.dropped_exchange_prompts.is_empty() {
        return Ok(GuardResult::None);
    }
    let head_content = rc.head_content();
    let head = head_content
        .as_deref()
        .map(String::as_str)
        .unwrap_or_default();
    let still_missing: Vec<String> = state
        .dropped_exchange_prompts
        .iter()
        .filter(|prompt| !exchange_contains_prompt_line(head, prompt))
        .cloned()
        .collect();
    if still_missing.is_empty() {
        // The dropped prompt reached the committed document — resolved.
        crate::cycle_state::clear_dropped_exchange_prompts(file)?;
        return Ok(GuardResult::None);
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "dropped_exchange_prompt_guard_failed file={} count={}",
            file.display(),
            still_missing.len()
        ),
    );
    Ok(GuardResult::Error(format!(
        "[session-check] INTERRUPTED: user-authored exchange prompt(s) were dropped during an IPC content_ours merge and are missing from the committed document: {}. The cycle committed `content_ours` without these prompt-bearing line(s); re-add them to `agent:exchange` and re-run `agent-doc finalize {}` / `agent-doc write --commit {}` so they are answered (see #exchange-prompt-dropped-on-merge).",
        still_missing.join("; "),
        file.display(),
        file.display()
    )))
}

pub(crate) fn check_completed_pending_reap_guard(
    _file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<Option<String>> {
    // Phase 6 (#lr-content-6): cached content + parsed components.
    let content = rc.doc_content();
    let components = rc.components();
    let completed: Vec<crate::pending::PendingItem> = components
        .iter()
        .filter(|component| is_tracked_work_component(&component.name))
        .flat_map(|component| completed_pending_items(component.content(&content)))
        .collect();
    if completed.is_empty() {
        return Ok(None);
    }

    let refs = completed
        .into_iter()
        .map(|item| {
            if item.id.is_empty() {
                format!("<missing-id> {}", item.text)
            } else {
                format!("#{}", item.id)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    if refs.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "[session-check] INTERRUPTED: document still contains completed tracked item(s) after closeout: {}. Re-run preflight/repair so the reap is persisted through the snapshot + commit boundary",
        refs
    )))
}

pub(crate) fn completed_pending_items(body: &str) -> Vec<crate::pending::PendingItem> {
    let (_, items, _) = crate::pending::parse_items(body);
    items
        .into_iter()
        .filter(crate::pending::PendingItem::is_done)
        .collect()
}

pub(crate) fn check_snapshot_committed_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    use crate::git::SnapshotCommitStatus;
    match rc.snapshot_commit_status() {
        SnapshotCommitStatus::Committed
        | SnapshotCommitStatus::NoSnapshot
        | SnapshotCommitStatus::NoHead
        | SnapshotCommitStatus::NotInGitRepo => Ok(GuardResult::None),
        SnapshotCommitStatus::SnapshotDiffersFromHead {
            snapshot_len,
            head_len,
        } => {
            // Phase 3 (#jbccc3): silently treat the auto-recoverable cancel
            // pattern as a non-error here. Standalone `session-check` is then
            // free to surface OK while preflight runs the binary-owned commit
            // through `enforce_no_uncommitted_closeout_drift`. Without this
            // skip, the guard would still bail with the misleading "cycle
            // state is committed but the snapshot does not match HEAD"
            // message that masks the JB cache-conflict cancel root cause.
            if detect_jb_cache_conflict_cancel_recoverable_with_context(file, rc)? {
                return Ok(GuardResult::None);
            }
            let side_effects = tracked_side_effect_note(file)?;
            let msg = format!(
                "[session-check] INTERRUPTED: cycle state is committed but the snapshot does not match HEAD in the owning repo (snapshot_len={}, head_len={}). The response patchback is visible but was never committed{} {}",
                snapshot_len,
                head_len,
                side_effects,
                closeout_recovery_hint(file)
            );
            eprintln!("{}", msg);
            crate::ops_log::log_op(
                file,
                &format!(
                    "snapshot_committed_guard_failed file={} snapshot_len={} head_len={}",
                    file.display(),
                    snapshot_len,
                    head_len
                ),
            );
            Ok(GuardResult::Error(msg))
        }
    }
}

pub(crate) fn closeout_recovery_hint(file: &Path) -> String {
    // `#closeout-repair-churn`: render one typed recovery instruction for the
    // classified state instead of a single static "try write --commit" line.
    let state = crate::flow::closeout::classify_closeout_recovery_state(file);
    match state.recovery_command(file) {
        Some(command) => format!("Recovery [{}]: {}.", state.as_str(), command),
        None => format!(
            "Use `agent-doc write --commit {}` once the visible response body is final, then re-run `agent-doc session-check {}`.",
            file.display(),
            file.display()
        ),
    }
}

/// `#codex-final-response-not-written`: a completed turn that committed real
/// binary-owned work this cycle but never captured an assistant response body.
///
/// Symptom: an agent (notably a Codex/direct-exec run, or any cycle whose
/// `finalize` landed pending mutations + the commit but lost the response — e.g.
/// a malformed/empty patchback) reaches `Committed` with side effects applied,
/// yet `agent:exchange` has no new `### Re:` close-out. The cycle-state proves
/// it: a real binary write turn sets `had_pending_mutations`, and a captured
/// response always sets `capture_id`/`response_sha256` (see
/// `capture::record` → `cycle_state::mark_response_captured`). So
/// `Committed` + `had_pending_mutations` + no `capture_id` means the write path
/// processed this turn's mutations and committed without ever persisting a
/// response — the missing close-out.
///
/// This is precise rather than broad: a no-op sweep close
/// (`closing cycle as already committed`) never sets `had_pending_mutations`,
/// and any normal response cycle sets `capture_id`, so neither false-fires.
/// Recovery is non-destructive — land the visible response through
/// `agent-doc write --commit`, which sets `capture_id` and clears the guard.
/// True when the committed `agent:exchange` contains at least one assistant
/// `### Re:` response heading (`#codex-queue-drain-no-response-body`). Used to
/// verify a queue-drain turn actually landed a response body in the document
/// rather than only mutating status/queue/backlog. A doc with no exchange
/// component, or an exchange holding only a compacted `### Session Summary`,
/// returns false.
pub(crate) fn committed_exchange_has_response_body(file: &Path) -> Result<bool> {
    let content = std::fs::read_to_string(file)?;
    let components = crate::component::parse(&content)?;
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return Ok(false);
    };
    let body = &content[exchange.open_end..exchange.close_start];
    Ok(body
        .lines()
        .any(|line| line.trim_start().starts_with("### Re:")))
}

pub(crate) fn check_committed_without_response_body_guard(file: &Path) -> Result<GuardResult> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if !matches!(state.phase, crate::cycle_state::CyclePhase::Committed) {
        return Ok(GuardResult::None);
    }
    let committed_exchange_has_body = committed_exchange_has_response_body(file)?;
    if committed_exchange_has_body {
        return Ok(GuardResult::None);
    }
    // A captured response (capture_id/response_sha256) normally means the
    // close-out body landed through the binary write path — not the
    // missing-response shape. EXCEPTION (#codex-queue-drain-no-response-body):
    // the systematic Codex queue-drain bug sets the capture record yet commits
    // only status/queue/backlog, leaving `agent:exchange` with zero `### Re:`
    // blocks. So for a queue-drain turn, capture metadata alone is not proof —
    // require the committed exchange to actually contain a response body before
    // trusting it. Non-queue turns keep trusting the capture record (no behavior
    // change); only a queue turn whose committed exchange has no `### Re:` body
    // falls through to fire.
    if state.capture_id.is_some() || state.response_sha256.is_some() {
        let is_queue_turn = state.queue_task_id.is_some() || !state.active_queue_heads.is_empty();
        if !is_queue_turn {
            return Ok(GuardResult::None);
        }
    }
    // Only fire when this cycle ran a response-write turn. `had_pending_mutations`
    // is set exclusively by the `write`/`finalize` response path
    // (`write.rs` → `cycle_state::mark_pending_mutations`), so it proves a real
    // response cycle processed mutations this turn. Bookkeeping-only commits stay
    // OK: a bare sweep re-commit never touches it, and `repair`'s completed-backlog
    // reap records `reaped_pending_ids` via `record_reaped_pending_ids` WITHOUT
    // `mark_pending_mutations` — a legitimate no-response commit that must not fire.
    if !state.had_pending_mutations {
        return Ok(GuardResult::None);
    }
    // A no-op commit (`commit_already_current`) committed NO new binary-owned work
    // this turn: the snapshot already equalled `HEAD`, so the pending mutation (a
    // reap / `--done` of an item already reflected in `HEAD`) left the document
    // byte-identical and there is nothing a paired response would accompany. Firing
    // here deadlocks the cycle — the recommended `write --commit` recovery is itself
    // a no-op (no response body to write, nothing to commit), so a re-running
    // closeout poller re-interrupts every pass forever. A real
    // `#codex-final-response-not-written` miss commits actual side-effect content
    // (`last_event` `commit_success` / `commit`), so it still fires.
    if crate::cycle_state::is_noop_commit_event(&state.last_event) {
        crate::ops_log::log_op(
            file,
            &format!(
                "committed_without_response_body_guard_skipped_noop_commit file={} cycle_id={} last_event={} pending_done={} reaped={}",
                file.display(),
                state.cycle_id,
                state.last_event,
                state.pending_done_ids.len(),
                state.reaped_pending_ids.len(),
            ),
        );
        return Ok(GuardResult::None);
    }
    let side_effects = tracked_side_effect_note(file)?;
    let msg = format!(
        "[session-check] INTERRUPTED: cycle committed binary-owned work this turn but no assistant `### Re:` response body is present in `agent:exchange` (cycle `{}`, last_event `{}`). The close-out response was never written into `agent:exchange`{} (#codex-queue-drain-no-response-body). {}",
        state.cycle_id,
        state.last_event,
        side_effects,
        closeout_recovery_hint(file)
    );
    eprintln!("{}", msg);
    crate::ops_log::log_op(
        file,
        &format!(
            "committed_without_response_body_guard_failed file={} cycle_id={} last_event={} had_pending_mutations={} pending_done={} reaped={}",
            file.display(),
            state.cycle_id,
            state.last_event,
            state.had_pending_mutations,
            state.pending_done_ids.len(),
            state.reaped_pending_ids.len(),
        ),
    );
    Ok(GuardResult::Error(msg))
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
use std::fs;
use std::io::Write;
use std::process::Command;
#[test]
fn committed_without_response_body_guard_passes_recovered_exchange_body_without_capture_metadata() {
    // Recovery may commit a visible `### Re:` after the original queue-drain
    // cycle lost its capture metadata. The committed exchange body is still
    // sufficient proof that the missing-response closeout has been repaired.
    let _lock = crate::test_support::env_lock();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
    let doc = root.join("doc.md");
    let content = concat!(
        "---\nagent_doc_session: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Session Summary\n\nCompacted.\n\n",
        "### Re: do [#ipc1] / do [#39c5]\n\nRecovered.\n",
        "<!-- /agent:exchange -->\n"
    )
    .to_string();
    fs::write(&doc, &content).unwrap();
    crate::snapshot::save(&doc, &content).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(&content), Some(&content)).unwrap();
    crate::cycle_state::mark_pending_mutations(&doc).unwrap();
    crate::cycle_state::record_pending_done_ids(&doc, &["ipc1".to_string(), "39c5".to_string()])
        .unwrap();
    crate::cycle_state::record_active_queue_heads(
        &doc,
        &["do [#ipc1]".to_string(), "do [#39c5]".to_string()],
    )
    .unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(&content), Some(&content))
        .unwrap();

    assert!(matches!(
        check_committed_without_response_body_guard(&doc).unwrap(),
        GuardResult::None
    ));
}
#[test]
fn committed_without_response_body_guard_skips_noop_commit_reap_only_cycle() {
    // Deadlock repro (tsift.md cycle-1780257680821): a `finalize --done X` whose
    // only effect was reaping an item already reflected in HEAD commits a no-op
    // (`commit_already_current`) and sets `had_pending_mutations`, but writes no
    // response body. The guard must NOT fire — a no-op commit committed no
    // binary-owned work this turn, so there is nothing a response would
    // accompany; firing wedges the cycle in an infinite
    // session-check-interrupted loop because the `write --commit` recovery is
    // itself a no-op. A real side-effect commit (`commit_success`) still fires.
    let _lock = crate::test_support::env_lock();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
    let doc = root.join("doc.md");
    let current =
        "---\nagent_doc_session: test\n---\n\n## Exchange\n\ndo [#nsga4verify]\n".to_string();
    fs::write(&doc, &current).unwrap();
    crate::snapshot::save(&doc, &current).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(&current), Some(&current)).unwrap();
    crate::cycle_state::mark_pending_mutations(&doc).unwrap();
    crate::cycle_state::record_pending_done_ids(&doc, &["nsga4verify".to_string()]).unwrap();
    crate::cycle_state::mark_committed(
        &doc,
        "commit_already_current",
        Some(&current),
        Some(&current),
    )
    .unwrap();
    assert!(matches!(
        check_committed_without_response_body_guard(&doc).unwrap(),
        GuardResult::None
    ));
}
}
