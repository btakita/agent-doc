//! # Module: repair
//!
//! ## Spec
//! - Guards against response loss caused by context compaction interrupting the write-back phase (between agent respond and `agent-doc write`).
//! - Pending responses are stored in `.agent-doc/pending/<hash>.md` before the write attempt, and
//!   the same response is also captured in `.agent-doc/captures/<doc-hash>/<cycle-id>.json`.
//! - `run(file)` — canonicalizes the path, checks for a pending file or a recoverable durable
//!   capture, and applies it if found. Terminal captures (`committed`, `discarded`) are ignored for
//!   replay so later preflights do not repeatedly enter the dedup path after a successful closeout.
//!   Before applying, reads the current document and checks if the response is already present
//!   (dedup guard). If already present, template docs still run binary-owned transcript/tail
//!   normalization before pending cleanup, then `run(file)` returns `RepairOutcome::AlreadyApplied`.
//!   When replaying from a durable capture, requires the current document and snapshot hashes to
//!   still match the captured baseline; otherwise fails closed.
//!   Template/CRDT documents always replay through the template write path
//!   (`write::apply_template_from_string`) even when the captured response is raw text
//!   without `<!-- patch:... -->` fences (for example `compact exchange` closeouts).
//!   Non-template documents use plain append (`write::apply_append_from_string`).
//!   Removes the pending file on successful write.
//! - Empty pending files are cleaned up without triggering a write; `run` returns `RepairOutcome::Noop`.
//! - `repair(file)` — runs the same recovery logic as `run(file)` and, when recovery work happened
//!   inside a git repo, immediately attempts `git::commit(file)` so the repaired response crosses
//!   the normal commit boundary instead of waiting for a later `preflight`.
//! - When there is no pending response/capture to replay, `run(file)` also reaps stale completed
//!   backlog items (`- [x] ...`) that should already have been removed, synchronizing the reap
//!   into the snapshot and `agent:pending-done` archive when present.
//! - `save_pending(file, response)` — writes the response to the pending store, creating parent directories as needed.
//! - `clear_pending(file)` — removes the pending file; no-op if it does not exist.
//! - `normalized_response_lines(response)` — extracts the response's non-empty, non-marker lines for dedup checking and normalizes transient ` (HEAD)` response-heading churn.
//!
//! ## Agentic Contracts
//! - `run(file)` — returns a `RepairOutcome` describing whether nothing happened, the response was replayed, the response was already present, manual tail cleanup was respected, or a stale `preflight_started` lock was repaired. Returns `Err` on I/O failure or if the write-back itself fails.
//! - `repair(file)` — preserves `run(file)` behavior and additionally attempts `git::commit(file)` when the document lives in git and the outcome was not `Noop`.
//! - Pending file is removed only after a fully successful write (or dedup detection); a failed write leaves the pending file intact for retry.
//! - `save_pending` and `clear_pending` are idempotent with respect to directory creation and missing files respectively.
//! - Callers (e.g., `preflight`) invoke `run` at session start to surface any orphaned responses before proceeding.
//!
//! ## Evals
//! - no_pending_returns_false: document with no pending file or capture → run returns Ok(false)
//! - save_and_clear_pending: save then clear → pending file created then removed
//! - recover_append_response: pending plain text response → applied as Assistant section, file updated, pending file removed, run returns Ok(true)
//! - empty_pending_cleaned_up: pending file with only whitespace → run returns Ok(false), pending file removed
//! - recover_skips_duplicate_apply: pending response already present in document → run returns Ok(false), pending file removed, document unchanged
//! - recover_already_applied_template_canonicalizes_prompt_prefixes: template dedup still restores missing `❯ ` transcript prefixes before cleanup
//! - recover_replays_capture_without_pending: durable capture with no pending file → run returns Ok(true)
//! - recover_fails_closed_on_capture_hash_mismatch: durable capture baseline mismatch → run returns Err

use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

use crate::{frontmatter, snapshot, write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairOutcome {
    Noop,
    ReplayedResponse,
    AlreadyApplied,
    ManualTailRemovalRespected,
    StalePreflightLockRepaired,
    CommitBoundaryRecovered,
    CompletedBacklogReaped,
}

impl RepairOutcome {
    pub fn repaired(self) -> bool {
        !matches!(self, Self::Noop)
    }

    pub fn replayed_response(self) -> bool {
        matches!(self, Self::ReplayedResponse)
    }
}

fn capture_is_repairable(capture: &crate::capture::CaptureRecord) -> bool {
    matches!(
        capture.state,
        crate::capture::CaptureState::Captured
            | crate::capture::CaptureState::WriteApplied
            | crate::capture::CaptureState::Replayed
    )
}

fn first_response_heading_line(response: &str) -> Option<&str> {
    response
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("### Re:"))
}

fn normalize_replay_topic(text: &str) -> String {
    let trimmed = text.trim();
    let trimmed = trimmed
        .strip_prefix("❯ ")
        .unwrap_or(trimmed)
        .strip_prefix("### Re:")
        .unwrap_or(trimmed)
        .trim();
    let trimmed = trimmed
        .split_once(" — ")
        .map(|(topic, _)| topic)
        .unwrap_or(trimmed)
        .trim();
    let trimmed = trimmed
        .strip_prefix("do ")
        .unwrap_or(trimmed)
        .trim_start_matches('#')
        .trim();

    let mut normalized = String::new();
    let mut last_was_space = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            last_was_space = false;
        } else if !last_was_space {
            normalized.push(' ');
            last_was_space = true;
        }
    }
    normalized.trim().to_string()
}

fn line_matches_historical_prompt(line: &str, topic: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("## Assistant")
        || trimmed.starts_with("<!--")
    {
        return false;
    }
    if !(trimmed.starts_with("❯ ")
        || trimmed.starts_with('#')
        || trimmed.starts_with("do #")
        || trimmed.starts_with("preset #"))
    {
        return false;
    }

    let normalized_line = normalize_replay_topic(trimmed);
    !normalized_line.is_empty()
        && (normalized_line == topic
            || normalized_line.contains(topic)
            || topic.contains(&normalized_line))
}

fn has_matching_orphan_prompt_for_committed_capture(
    doc_content: &str,
    response_heading: &str,
) -> bool {
    let topic = normalize_replay_topic(response_heading);
    if topic.is_empty() {
        return false;
    }

    let body = frontmatter::parse(doc_content)
        .map(|(_, body)| body)
        .unwrap_or(doc_content);
    let exchange = if let Ok(components) = crate::component::parse(body) {
        components
            .iter()
            .find(|component| component.name == "exchange")
            .map(|component| component.content(body).to_string())
            .unwrap_or_else(|| body.to_string())
    } else {
        body.to_string()
    };

    let mut saw_match = false;
    for line in exchange.lines() {
        let trimmed = line.trim();
        if trimmed == response_heading.trim() {
            return false;
        }
        if saw_match && trimmed.starts_with("### Re:") {
            return false;
        }
        if line_matches_historical_prompt(trimmed, &topic) {
            saw_match = true;
        }
    }

    saw_match
}

fn historical_committed_capture_replay(
    file: &Path,
    doc_content: &str,
) -> Result<Option<crate::capture::CaptureRecord>> {
    let Some(capture) = crate::capture::latest_committed(file)? else {
        return Ok(None);
    };
    if is_already_applied(doc_content, &capture.response_body) {
        return Ok(None);
    }
    let Some(response_heading) = first_response_heading_line(&capture.response_body) else {
        return Ok(None);
    };
    if !has_matching_orphan_prompt_for_committed_capture(doc_content, response_heading) {
        return Ok(None);
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "repair_replay_committed_capture file={} capture_id={}",
            file.display(),
            capture.capture_id
        ),
    );
    Ok(Some(capture))
}

pub(crate) const AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR: &str =
    "ambiguous preflight_started patchback";

fn normalized_content_hash(content: &str) -> String {
    crate::ops_log::content_hash(&crate::git::normalize_transient_agent_doc_markers(content))
}

fn repair_stale_preflight_started_cycle(file: &Path) -> Result<RepairOutcome> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(RepairOutcome::Noop);
    };
    if state.phase != crate::cycle_state::CyclePhase::PreflightStarted {
        return Ok(RepairOutcome::Noop);
    }

    let file_content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read {} for stale preflight repair",
            file.display()
        )
    })?;
    let snapshot_content = snapshot::load(file)?;
    let current_file_hash = crate::ops_log::content_hash(&file_content);
    let current_snapshot_hash = snapshot_content
        .as_deref()
        .map(crate::ops_log::content_hash);
    let current_normalized_file_hash = normalized_content_hash(&file_content);
    let current_normalized_snapshot_hash = snapshot_content.as_deref().map(normalized_content_hash);

    let raw_hashes_match = state.file_hash.as_deref() == Some(current_file_hash.as_str())
        && state.snapshot_hash == current_snapshot_hash;
    let normalized_hashes_match = state.normalized_file_hash.as_deref()
        == Some(current_normalized_file_hash.as_str())
        && state.normalized_snapshot_hash == current_normalized_snapshot_hash;

    if raw_hashes_match || normalized_hashes_match {
        crate::cycle_state::mark_committed(
            file,
            "repair_preflight_stale_lock",
            snapshot_content.as_deref(),
            Some(&file_content),
        )?;
        crate::ops_log::log_op(
            file,
            &format!(
                "repair_preflight_stale_lock file={} cycle_id={}",
                file.display(),
                state.cycle_id
            ),
        );
        eprintln!(
            "[repair] repaired stale preflight_started cycle {} for {}",
            state.cycle_id,
            file.display()
        );
        return Ok(RepairOutcome::StalePreflightLockRepaired);
    }

    if let Some(reason) = crate::git::repair_committed_historical_snapshot_drift(file)? {
        let repaired_snapshot = snapshot::load(file)?;
        crate::cycle_state::mark_committed(
            file,
            "repair_preflight_committed_historical",
            repaired_snapshot.as_deref(),
            Some(&file_content),
        )?;
        crate::capture::mark_committed(file)?;
        crate::ops_log::log_op(
            file,
            &format!(
                "repair_preflight_committed_historical file={} cycle_id={} reason={}",
                file.display(),
                state.cycle_id,
                reason
            ),
        );
        eprintln!(
            "[repair] closed stale preflight_started cycle {} for {} after repairing committed historical {} drift",
            state.cycle_id,
            file.display(),
            reason
        );
        return Ok(RepairOutcome::StalePreflightLockRepaired);
    }

    if let Some(marker) = crate::session_check::detect_bypassed_response_write(file)? {
        anyhow::bail!(
            "{} for {}: found visible response patchback ({marker}) but no pending/capture artifact exists and HEAD cannot prove the patchback was already committed",
            AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR,
            file.display(),
        );
    }

    Ok(RepairOutcome::Noop)
}

pub(crate) fn recover_missing_commit_boundary(
    file: &Path,
    event: &str,
) -> Result<Option<&'static str>> {
    let state = crate::cycle_state::load(file)?;
    let has_open_commit_boundary = state.as_ref().is_some_and(|state| {
        matches!(
            state.phase,
            crate::cycle_state::CyclePhase::ResponseCaptured
                | crate::cycle_state::CyclePhase::WriteApplied
        )
    });
    let has_missing_commit_event = if has_open_commit_boundary {
        false
    } else {
        crate::session_check::detect_write_completed_commit_missing(file)?.is_some()
    };
    if !has_open_commit_boundary && !has_missing_commit_event {
        return Ok(None);
    }

    let current_doc = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read {} for commit-boundary recovery",
            file.display()
        )
    })?;
    let head_doc = crate::git::show_head(file)?;
    let reason = match crate::git::verify_snapshot_committed(file)? {
        crate::git::SnapshotCommitStatus::Committed => head_doc
            .as_deref()
            .filter(|head| {
                crate::session_check::detect_bypassed_response_write_between(head, &current_doc)
                    .is_none()
            })
            .map(|_| "already-committed HEAD"),
        _ => crate::git::repair_committed_historical_snapshot_drift(file)?
            .map(|_| "committed historical exchange snapshot drift"),
    };
    let Some(reason) = reason else {
        return Ok(None);
    };

    let repaired_snapshot = snapshot::load(file)?;
    crate::cycle_state::mark_committed(
        file,
        event,
        repaired_snapshot.as_deref(),
        Some(&current_doc),
    )?;
    crate::capture::mark_committed(file)?;
    crate::ops_log::log_op(
        file,
        &format!(
            "repair_commit_boundary_recovered file={} event={} reason={}",
            file.display(),
            event,
            reason
        ),
    );
    Ok(Some(reason))
}

fn repair_completed_backlog_items(file: &Path) -> Result<RepairOutcome> {
    let content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read {} for completed backlog reap repair",
            file.display()
        )
    })?;
    let components = crate::component::parse(&content).with_context(|| {
        format!(
            "failed to parse {} for completed backlog reap repair",
            file.display()
        )
    })?;
    let Some(backlog) = components
        .iter()
        .find(|component| crate::component::is_backlog_component(&component.name))
    else {
        return Ok(RepairOutcome::Noop);
    };

    let doc_id = crate::pending_cmd::doc_id_for(file);
    let (canonical_body, _) =
        crate::pending::backfill(backlog.content(&content), &doc_id, &HashSet::new());
    let (new_body, removed) = crate::pending::reap_with_items(&canonical_body)?;
    if removed.is_empty() {
        return Ok(RepairOutcome::Noop);
    }

    let mut repaired = backlog.replace_content(&content, &new_body);
    if let Some(archived) = crate::preflight::archive_pending_done(&repaired, &removed) {
        repaired = archived;
    }

    write::atomic_write_pub(file, &repaired)?;

    let repaired_snapshot = if let Some(snap_content) = snapshot::load(file)? {
        let snap_components = crate::component::parse(&snap_content).with_context(|| {
            format!(
                "failed to parse snapshot for completed backlog reap repair {}",
                file.display()
            )
        })?;
        let snap_backlog = snap_components
            .iter()
            .find(|component| crate::component::is_backlog_component(&component.name))
            .with_context(|| {
                format!(
                    "completed backlog reap repair requires the snapshot backlog component in {}",
                    file.display()
                )
            })?;

        let mut new_snapshot = snap_backlog.replace_content(&snap_content, &new_body);
        if let Some(archived) = crate::preflight::archive_pending_done(&new_snapshot, &removed) {
            new_snapshot = archived;
        }
        snapshot::save(file, &new_snapshot)?;
        Some(new_snapshot)
    } else {
        None
    };

    if repaired_snapshot.as_deref() == Some(repaired.as_str()) {
        let _ = crate::cycle_state::mark_committed(
            file,
            "repair_completed_backlog_reap",
            Some(&repaired),
            Some(&repaired),
        );
    }

    let refs = removed
        .iter()
        .map(|item| format!("#{}", item.id))
        .collect::<Vec<_>>()
        .join(", ");
    crate::ops_log::log_op(
        file,
        &format!(
            "repair_completed_backlog_reap file={} count={} ids={}",
            file.display(),
            removed.len(),
            refs
        ),
    );
    eprintln!(
        "[repair] reaped stale completed backlog item(s) in {}: {}",
        file.display(),
        refs
    );

    Ok(RepairOutcome::CompletedBacklogReaped)
}

fn repair_template_doc_if_needed(file: &Path, doc_content: &str) -> Result<String> {
    let tail_repaired = crate::template::repair_conversation_tail_outside_exchange(doc_content)?
        .unwrap_or_else(|| doc_content.to_string());
    let tail_changed = tail_repaired != doc_content;

    let (fm, _) = frontmatter::parse(&tail_repaired)
        .with_context(|| format!("failed to parse document frontmatter {}", file.display()))?;

    let mut repaired = tail_repaired.clone();
    if fm.resolve_mode().is_template()
        && let Some(snapshot_content) = snapshot::load(file)?
    {
        repaired = write::normalize_user_prompts_in_exchange_safe(
            &repaired,
            &repaired,
            &snapshot_content,
            file,
        );
        repaired = write::normalize_template_structure_or_fail(&repaired, file)?;
    }
    let prompt_changed = repaired != tail_repaired;

    if tail_changed || prompt_changed {
        write::atomic_write_pub(file, &repaired)?;
        snapshot::save(file, &repaired)?;
        if tail_changed {
            crate::ops_log::log_op(
                file,
                &format!("repair_exchange_tail file={}", file.display()),
            );
            eprintln!(
                "[repair] repaired escaped conversation tail in {}",
                file.display()
            );
        }
        if prompt_changed {
            crate::ops_log::log_op(
                file,
                &format!("repair_prompt_prefixes file={}", file.display()),
            );
            eprintln!(
                "[repair] repaired transcript prompt prefixes in {}",
                file.display()
            );
        }
    }

    Ok(repaired)
}

fn same_content_ignoring_trailing_newlines(left: &str, right: &str) -> bool {
    left.trim_end_matches('\n') == right.trim_end_matches('\n')
}

#[derive(Debug, Serialize)]
struct BlockedRepairPayloadRecord<'a> {
    captured_at: u64,
    file: String,
    reason: &'a str,
    payload_sha256: String,
    response_body: &'a str,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn save_blocked_repair_payload(
    file: &Path,
    response: &str,
    reason: &str,
) -> Result<std::path::PathBuf> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root = crate::snapshot::find_project_root(&canonical)
        .or_else(|| canonical.parent().map(Path::to_path_buf))
        .context("resolve project root for blocked repair payload")?;
    let dir = root.join(".agent-doc/repair-blocked");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create blocked repair dir {}", dir.display()))?;
    let filename = format!(
        "{}-{}.json",
        crate::ops_log::content_hash(canonical.to_string_lossy().as_ref()),
        now_millis()
    );
    let path = dir.join(filename);
    let record = BlockedRepairPayloadRecord {
        captured_at: now_secs(),
        file: canonical.display().to_string(),
        reason,
        payload_sha256: crate::ops_log::content_hash(response),
        response_body: response,
    };
    let json = serde_json::to_string_pretty(&record)?;
    std::fs::write(&path, json)
        .with_context(|| format!("write blocked repair payload {}", path.display()))?;
    Ok(path)
}

fn fail_closed_on_blocked_template_replay(file: &Path, response: &str, reason: &str) -> Result<()> {
    match save_blocked_repair_payload(file, response, reason) {
        Ok(path) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "repair_blocked_replay path={} reason={}",
                    path.display(),
                    reason
                ),
            );
            anyhow::bail!(
                "refused to replay pending response for {} because {}; blocked payload captured at {}",
                file.display(),
                reason,
                path.display()
            );
        }
        Err(err) => {
            anyhow::bail!(
                "refused to replay pending response for {} because {}; additionally failed to save blocked payload: {}",
                file.display(),
                reason,
                err
            );
        }
    }
}

fn discard_pending_capture_for_manual_repair(file: &Path, current_doc: &str) -> Result<()> {
    let pending_path = snapshot::pending_path_for(file)?;
    if pending_path.exists() {
        std::fs::remove_file(&pending_path).with_context(|| {
            format!(
                "failed to remove pending response after manual repair {}",
                pending_path.display()
            )
        })?;
    }
    if let Err(e) = snapshot::delete_pre_response(file) {
        eprintln!("[repair] warning: failed to delete pre-response: {}", e);
    }
    crate::capture::mark_discarded(file)?;
    snapshot::save(file, current_doc)?;
    crate::cycle_state::mark_committed(
        file,
        "repair_respect_manual_exchange_tail_removal",
        Some(current_doc),
        Some(current_doc),
    )?;
    crate::ops_log::log_op(
        file,
        &format!(
            "repair_discard_stale_capture_after_manual_tail_removal file={}",
            file.display()
        ),
    );
    eprintln!(
        "[repair] respected manual removal of escaped conversation tail in {}",
        file.display()
    );
    Ok(())
}

fn respect_manual_exchange_tail_removal_if_safe(
    file: &Path,
    doc_content: &str,
    capture: &crate::capture::CaptureRecord,
) -> Result<bool> {
    let (fm, _) = frontmatter::parse(doc_content)
        .with_context(|| format!("failed to parse document frontmatter {}", file.display()))?;
    if !fm.resolve_mode().is_template() {
        return Ok(false);
    }

    let Some(snapshot_content) = snapshot::load(file)? else {
        return Ok(false);
    };
    if capture.snapshot_hash != Some(crate::ops_log::content_hash(&snapshot_content)) {
        return Ok(false);
    }

    let Some(stripped_snapshot) =
        crate::template::strip_conversation_tail_outside_exchange(&snapshot_content)?
    else {
        return Ok(false);
    };
    if !same_content_ignoring_trailing_newlines(&stripped_snapshot, doc_content) {
        return Ok(false);
    }

    discard_pending_capture_for_manual_repair(file, doc_content)?;
    Ok(true)
}

/// Check for a pending response and apply it if found.
pub fn run(file: &Path) -> Result<RepairOutcome> {
    // Canonicalize first to handle CWD drift (e.g., when CWD is in a submodule)
    let canonical = file
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("file not found: {}", file.display()))?;

    let pending_path = snapshot::pending_path_for(&canonical)?;
    let capture = crate::capture::load_active(&canonical)?.filter(capture_is_repairable);
    let doc_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read document for repair {}", file.display()))?;
    let historical_capture = if !pending_path.exists() && capture.is_none() {
        historical_committed_capture_replay(&canonical, &doc_content)?
    } else {
        None
    };
    if !pending_path.exists() && capture.is_none() && historical_capture.is_none() {
        let outcome = repair_stale_preflight_started_cycle(file)?;
        if outcome != RepairOutcome::Noop {
            return Ok(outcome);
        }
        if recover_missing_commit_boundary(file, "repair_commit_boundary_recovered")?.is_some() {
            return Ok(RepairOutcome::CommitBoundaryRecovered);
        }
        return repair_completed_backlog_items(file);
    }

    let pending_response = if pending_path.exists() {
        Some(std::fs::read_to_string(&pending_path).with_context(|| {
            format!("failed to read pending response {}", pending_path.display())
        })?)
    } else {
        None
    };
    let response = capture
        .as_ref()
        .map(|r| r.response_body.clone())
        .or_else(|| historical_capture.as_ref().map(|r| r.response_body.clone()))
        .or(pending_response.clone())
        .unwrap_or_default();

    if response.trim().is_empty() {
        // Empty pending file — just clean up
        let _ = std::fs::remove_file(&pending_path);
        let _ = crate::capture::mark_discarded(&canonical);
        return Ok(RepairOutcome::Noop);
    }

    // Dedup guard: check if the response content is already present in the document.
    // This prevents double-apply when the pending file was left behind after a successful
    // IPC write (e.g., IPC timeout path exits with code 75 without calling clear_pending,
    // but the plugin already applied the content via the IPC patch file).
    if is_already_applied(&doc_content, &response) {
        eprintln!(
            "[repair] Response already present in document — skipping apply, cleaning up pending file"
        );
        let repaired_doc = repair_template_doc_if_needed(file, &doc_content)?;
        let state_is_open = crate::cycle_state::load(file)?
            .map(|state| state.is_open())
            .unwrap_or(true);
        let snapshot_missing_response = snapshot::load(file)?
            .as_deref()
            .map(|snapshot_doc| !is_already_applied(snapshot_doc, &response))
            .unwrap_or(true);
        if state_is_open && snapshot_missing_response {
            snapshot::save(file, &repaired_doc)?;
            crate::ops_log::log_op(
                file,
                &format!(
                    "repair_adopt_existing_response file={} reason=snapshot_missing_response",
                    file.display()
                ),
            );
            eprintln!(
                "[repair] advanced snapshot to the already-present response for {}",
                file.display()
            );
        }
        if state_is_open
            && let Err(e) = crate::cycle_state::mark_write_applied(
                file,
                "repair_already_applied",
                Some(&repaired_doc),
                Some(&repaired_doc),
            )
        {
            eprintln!("[repair] cycle-state update failed: {} (non-fatal)", e);
        }
        clear_pending(&canonical)?;
        return Ok(RepairOutcome::AlreadyApplied);
    }

    if let Some(ref capture) = capture {
        if respect_manual_exchange_tail_removal_if_safe(&canonical, &doc_content, capture)? {
            return Ok(RepairOutcome::ManualTailRemovalRespected);
        }
        crate::capture::validate_replay(&canonical, capture)?;
    }

    eprintln!(
        "[repair] Found orphaned response for {} ({} bytes). Applying...",
        file.display(),
        response.len()
    );

    let (fm, _) = frontmatter::parse(&doc_content)
        .with_context(|| format!("failed to parse document frontmatter {}", file.display()))?;
    let use_template_write = fm.resolve_mode().is_template() || response.contains("<!-- patch:");
    if use_template_write
        && let crate::replay_guard::ReplayPayloadClassification::Blocked(reason) =
            crate::replay_guard::classify_replay_payload(&response)
    {
        fail_closed_on_blocked_template_replay(file, &response, &reason)?;
    }
    if use_template_write {
        write::apply_template_from_string(file, &response)?;
    } else {
        write::apply_append_from_string(file, &response)?;
    }

    // Remove the pending file after successful write
    clear_pending(&canonical)?;

    eprintln!(
        "[repair] Response repaired and written to {}",
        file.display()
    );
    let final_doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read recovered document {}", file.display()))?;
    if let Err(e) = crate::cycle_state::mark_write_applied(
        file,
        "repair_applied",
        Some(&final_doc),
        Some(&final_doc),
    ) {
        eprintln!("[repair] cycle-state update failed: {} (non-fatal)", e);
    }
    if historical_capture.is_none()
        && let Err(e) = crate::capture::mark_replayed(&canonical)
    {
        eprintln!("[repair] capture-state update failed: {} (non-fatal)", e);
    }
    Ok(RepairOutcome::ReplayedResponse)
}

pub fn repair(file: &Path) -> Result<RepairOutcome> {
    let outcome = run(file)?;
    if outcome.repaired() && crate::git::is_in_git_repo(file) {
        crate::write::complete_required_closeout(file)?;
    }
    Ok(outcome)
}

/// Returns true if the pending response content appears to already be applied to the document.
///
/// Checks whether the document contains the response's normalized visible lines
/// as one contiguous block. This tolerates blank-line separation and transient
/// ` (HEAD)` suffixes on response headings without treating scattered matching
/// phrases elsewhere in the document as an already-applied replay.
fn is_already_applied(doc: &str, response: &str) -> bool {
    let response_lines = normalized_response_lines(response);
    if response_lines.is_empty() {
        return false;
    }
    let doc_lines = normalized_response_lines(doc);
    doc_lines
        .windows(response_lines.len())
        .any(|window| window == response_lines.as_slice())
}

fn normalized_response_lines(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(normalize_response_line)
        .collect()
}

fn normalize_response_line(line: &str) -> Option<String> {
    let raw = line.trim_end_matches('\r');
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("<!-- patch:")
        || trimmed.starts_with("<!-- /patch:")
        || trimmed.starts_with("<!-- agent:")
        || trimmed.starts_with("<!-- /agent:")
    {
        return None;
    }
    Some(strip_transient_response_head_marker(raw))
}

fn strip_transient_response_head_marker(line: &str) -> String {
    if let Some(stripped) = line.strip_suffix(" (HEAD)") {
        let trimmed = stripped.trim_start();
        let is_re_heading = trimmed.starts_with("### Re:");
        let is_bold_re_heading = trimmed.starts_with("**Re:") && trimmed.ends_with("**");
        if is_re_heading || is_bold_re_heading {
            return stripped.to_string();
        }
    }
    line.to_string()
}

/// Save a response to the pending store before attempting write-back.
/// This makes the response durable across context compaction.
pub fn save_pending(file: &Path, response: &str) -> Result<()> {
    crate::capture::capture_response(file, response)?;
    let pending_path = snapshot::pending_path_for(file)?;
    if let Some(parent) = pending_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&pending_path, response)
        .with_context(|| format!("failed to save pending response {}", pending_path.display()))?;
    Ok(())
}

/// Remove the pending file after a successful write-back.
pub fn clear_pending(file: &Path) -> Result<()> {
    let pending_path = snapshot::pending_path_for(file)?;
    if pending_path.exists() {
        std::fs::remove_file(&pending_path)?;
    }
    // Also clean up the pre-response snapshot (saved before write for undo support).
    // Without this, pre-response files accumulate indefinitely after successful writes.
    if let Err(e) = snapshot::delete_pre_response(file) {
        eprintln!("[repair] warning: failed to delete pre-response: {}", e);
    }
    if let Err(e) = crate::capture::mark_write_applied(file) {
        eprintln!("[repair] warning: failed to update capture state: {}", e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command as ProcessCommand;
    use tempfile::TempDir;

    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/pending")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
        dir
    }

    fn init_git_repo(root: &Path, tracked: &Path) {
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["init"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", tracked.file_name().unwrap().to_str().unwrap()])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .status()
            .unwrap();
    }

    #[test]
    fn no_pending_returns_false() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "# Doc\n\n## User\n\nHello\n").unwrap();
        assert_eq!(run(&doc).unwrap(), RepairOutcome::Noop);
    }

    #[test]
    fn save_and_clear_pending() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "content").unwrap();

        save_pending(&doc, "response text").unwrap();
        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(pending.exists());

        clear_pending(&doc).unwrap();
        assert!(!pending.exists());
    }

    #[test]
    fn repair_reaps_completed_backlog_without_pending_response() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] [#aaaa] keep\n",
            "- [x] [#bbbb] drop\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:pending-done -->\n",
            "<!-- /agent:pending-done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::CompletedBacklogReaped);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(repaired.contains("- [ ] [#aaaa] keep"));
        assert!(!repaired.contains("- [x] [#bbbb] drop"));
        assert!(repaired.contains("[#bbbb] drop"));

        let repaired_snapshot = snapshot::load(&doc).unwrap().unwrap();
        assert!(repaired_snapshot.contains("- [ ] [#aaaa] keep"));
        assert!(!repaired_snapshot.contains("- [x] [#bbbb] drop"));
        assert!(repaired_snapshot.contains("[#bbbb] drop"));
    }

    #[test]
    fn repair_backfills_legacy_done_ids_before_reaping_completed_backlog() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep\n",
            "- [x] legacy drop\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:pending-done -->\n",
            "<!-- /agent:pending-done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::CompletedBacklogReaped);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        let pending_body = repaired
            .split("<!-- agent:pending -->\n")
            .nth(1)
            .and_then(|rest| rest.split("\n<!-- /agent:pending -->").next())
            .expect("pending component");
        assert!(
            repaired.contains("- [ ] [#"),
            "open legacy item should be backfilled: {repaired}"
        );
        assert!(repaired.contains("keep"));
        assert!(!pending_body.contains("legacy drop"));
        assert!(repaired.contains("legacy drop"));

        let repaired_snapshot = snapshot::load(&doc).unwrap().unwrap();
        let snapshot_pending_body = repaired_snapshot
            .split("<!-- agent:pending -->\n")
            .nth(1)
            .and_then(|rest| rest.split("\n<!-- /agent:pending -->").next())
            .expect("snapshot pending component");
        assert!(repaired_snapshot.contains("- [ ] [#"));
        assert!(!snapshot_pending_body.contains("legacy drop"));
        assert!(repaired_snapshot.contains("legacy drop"));
        assert!(repaired_snapshot.contains("pending-done"));
    }

    #[test]
    fn repair_commits_reaped_completed_backlog_in_git_repo() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] [#aaaa] keep\n",
            "- [x] [#bbbb] drop\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:pending-done -->\n",
            "<!-- /agent:pending-done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        init_git_repo(dir.path(), &doc);

        let outcome = repair(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::CompletedBacklogReaped);

        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(_) => {}
            other => panic!("expected clean closeout after repair, got {other:?}"),
        }

        let head = ProcessCommand::new("git")
            .current_dir(dir.path())
            .args(["show", "HEAD:test.md"])
            .output()
            .unwrap();
        let head_text = String::from_utf8_lossy(&head.stdout);
        assert!(head_text.contains("- [ ] [#aaaa] keep"));
        assert!(!head_text.contains("- [x] [#bbbb] drop"));
        assert!(head_text.contains("[#bbbb] drop"));
    }

    #[test]
    fn repair_completed_backlog_reap_preserves_live_prompt_outside_snapshot() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let snapshot_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [x] [#bbbb] drop\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:pending-done -->\n",
            "<!-- /agent:pending-done -->\n"
        );
        let live_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "do #statusws. spec-test-build-install-commit-push\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [x] [#bbbb] drop\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:pending-done -->\n",
            "<!-- /agent:pending-done -->\n"
        );
        std::fs::write(&doc, live_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::CompletedBacklogReaped);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(repaired.contains("do #statusws. spec-test-build-install-commit-push"));
        assert!(!repaired.contains("- [x] [#bbbb] drop"));

        let repaired_snapshot = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            !repaired_snapshot.contains("do #statusws. spec-test-build-install-commit-push"),
            "snapshot must not absorb the live prompt"
        );
        assert!(!repaired_snapshot.contains("- [x] [#bbbb] drop"));

        let diff = crate::diff::compute(&doc).unwrap().unwrap();
        assert!(diff.contains("do #statusws. spec-test-build-install-commit-push"));
    }

    #[test]
    fn recover_append_response() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\nagent_doc_mode: append\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();

        // Save a pending response
        save_pending(&doc, "This is the recovered response.").unwrap();

        // Recover it
        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        // Verify the response was written
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("This is the recovered response."));
        assert!(result.contains("## Assistant"));

        // Pending file should be cleaned up
        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists());
    }

    #[test]
    fn recover_plain_response_uses_template_path_for_template_docs() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "## User\n",
            "compact exchange\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        save_pending(
            &doc,
            "Exchange compacted. No new work was run in this turn.",
        )
        .unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        let result = std::fs::read_to_string(&doc).unwrap();
        let exchange_close = result.find("<!-- /agent:exchange -->").unwrap();
        let summary = result
            .find("Exchange compacted. No new work was run in this turn.")
            .unwrap();
        assert!(
            summary < exchange_close,
            "plain recovery for template docs should stay inside exchange:\n{result}"
        );
        assert!(
            !result[exchange_close..].contains("## Assistant"),
            "template recovery must not append inline assistant blocks after exchange:\n{result}"
        );
    }

    #[test]
    fn recover_normalizes_captured_replace_pending_patch() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] [#aaaa] existing\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: topic — gpt-5\n\n",
            "Recovered.\n",
            "<!-- /patch:exchange -->\n",
            "<!-- replace:pending -->\n",
            "- [x] [#aaaa] existing\n",
            "- [ ] [#bbbb] add regression coverage\n",
            "<!-- /replace:pending -->\n"
        );
        save_pending(&doc, response).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("### Re: topic — gpt-5"));
        assert!(result.contains("- [x] [#aaaa] existing"));
        assert!(result.contains("- [ ] [#bbbb] add regression coverage"));
        assert!(!result.contains("replace:pending"));
    }

    #[test]
    fn empty_pending_cleaned_up() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "content").unwrap();

        save_pending(&doc, "").unwrap();
        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::Noop);

        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists());
    }

    #[test]
    fn recover_skips_duplicate_apply() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        // Document already contains the response content (as if IPC applied it)
        let response = "This is the response that was already applied.\nSecond line.\nThird line.";
        let content = format!(
            "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\n{}\n\n## User\n\n",
            response
        );
        std::fs::write(&doc, &content).unwrap();

        // Pending file still exists (clear_pending was never called after IPC write)
        save_pending(&doc, response).unwrap();

        // run should detect the content is already present and skip
        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::AlreadyApplied);

        // Document should be unchanged
        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, content);

        // Pending file should be cleaned up
        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists());
    }

    #[test]
    fn recover_replays_capture_without_pending() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        save_pending(&doc, "Recovered from capture.").unwrap();
        clear_pending(&doc).unwrap();
        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists());
        // Re-arm capture as if the write never happened.
        crate::capture::capture_response(&doc, "Recovered from capture.").unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("Recovered from capture."));
    }

    #[test]
    fn recover_already_applied_template_canonicalizes_prompt_prefixes() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let snapshot = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Prior question?\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let response =
            "<!-- patch:exchange -->\n### Re: topic — gpt-5\n\nBody\n<!-- /patch:exchange -->";
        let current = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Prior question?\n",
            "Why was this missed?\n",
            "### Re: topic — gpt-5\n\n",
            "Body\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, snapshot).unwrap();

        save_pending(&doc, response).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::AlreadyApplied);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(
            repaired.contains("❯ Why was this missed?"),
            "repair should restore the missing prompt prefix:\n{repaired}"
        );
        assert!(
            !repaired.contains("\nWhy was this missed?\n"),
            "bare prompt target should not remain after repair:\n{repaired}"
        );

        let saved_snapshot = snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            saved_snapshot, repaired,
            "snapshot should advance to the canonicalized repaired document"
        );
    }

    #[test]
    fn recover_fails_closed_on_capture_hash_mismatch() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        save_pending(&doc, "Recovered from capture.").unwrap();
        let pending = snapshot::pending_path_for(&doc).unwrap();
        std::fs::remove_file(&pending).unwrap();
        std::fs::write(&doc, "---\nsession: test\n---\n\n## User\n\nHello again\n").unwrap();

        let err = run(&doc).unwrap_err();
        assert!(
            err.to_string().contains("baseline no longer matches"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn recover_respects_manual_removal_of_escaped_exchange_tail() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let malformed = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep\n",
            "<!-- /agent:pending -->\n\n",
            "[//]: # (leave this note outside exchange)\n\n",
            "## Assistant\n\n",
            "Escaped answer.\n"
        );
        std::fs::write(&doc, malformed).unwrap();
        snapshot::save(&doc, malformed).unwrap();

        save_pending(&doc, "Escaped answer.").unwrap();

        let repaired = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep\n",
            "<!-- /agent:pending -->\n\n",
            "[//]: # (leave this note outside exchange)\n"
        );
        std::fs::write(&doc, repaired).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(
            recovered,
            RepairOutcome::ManualTailRemovalRespected,
            "manual deletion of the escaped tail should be treated as a repair"
        );

        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, repaired);
        assert!(
            !result.contains("## Assistant"),
            "stale assistant tail must not be re-added:\n{result}"
        );

        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists(), "pending file should be cleared");

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
        assert_eq!(
            state.last_event,
            "repair_respect_manual_exchange_tail_removal"
        );

        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(snap, repaired, "snapshot should follow the user repair");

        let capture = crate::capture::load_active(&doc).unwrap().unwrap();
        assert_eq!(capture.state, crate::capture::CaptureState::Discarded);
    }

    #[test]
    fn recover_dedup_with_blank_lines_and_boundary() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        // Response has template patch with content lines
        let response = "<!-- patch:exchange -->\n### Re: topic — opus-4-6\n\n**Details:**\n- Item one\n<!-- /patch:exchange -->";
        // Document has the content with blank lines and (HEAD) boundary suffix
        let content = "---\nsession: test\n---\n\n<!-- agent:exchange -->\n### Re: topic — opus-4-6 (HEAD)\n\n**Details:**\n- Item one\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();

        save_pending(&doc, response).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(
            recovered,
            RepairOutcome::AlreadyApplied,
            "should detect content as already applied despite (HEAD) suffix and blank lines"
        );

        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn dedup_requires_contiguous_normalized_response_block() {
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: topic — opus-4-6\n",
            "Implemented in `src/agent-doc`.\n",
            "- `cargo test`\n",
            "<!-- /patch:exchange -->\n"
        );
        let doc = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: topic — opus-4-6 (HEAD)\n",
            "Earlier answer.\n\n",
            "Implemented in `src/agent-doc`.\n\n",
            "Unrelated text.\n",
            "- `cargo test`\n",
            "<!-- /agent:exchange -->\n"
        );

        assert!(
            !is_already_applied(doc, response),
            "scattered matching lines should not trigger dedup"
        );
    }

    #[test]
    fn dedup_short_response_still_requires_contiguous_match() {
        let response = "Implemented.\nDone.\n";
        let doc = "Implemented.\nOther line.\nDone.\n";

        assert!(
            !is_already_applied(doc, response),
            "short responses should not dedup from non-contiguous matches"
        );
    }

    #[test]
    fn recover_repairs_stale_preflight_started_cycle_when_hashes_match() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\nbody\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let repaired = run(&doc).unwrap();
        assert_eq!(
            repaired,
            RepairOutcome::StalePreflightLockRepaired,
            "stale preflight lock should be repaired"
        );
        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
        assert_eq!(state.last_event, "repair_preflight_stale_lock");
    }

    #[test]
    fn recover_repairs_preflight_started_cycle_when_committed_patchback_is_visible() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        init_git_repo(root, &doc);
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let updated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: topic — gpt-5\n",
            "Recovered body.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, updated).unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", "test.md"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .status()
            .unwrap();

        let repaired = run(&doc).unwrap();
        assert_eq!(repaired, RepairOutcome::StalePreflightLockRepaired);

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
        assert_eq!(state.last_event, "repair_preflight_committed_historical");
        assert_eq!(snapshot::load(&doc).unwrap().as_deref(), Some(updated));
    }

    #[test]
    fn recover_closes_write_applied_cycle_when_head_already_has_exchange_patchback() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        init_git_repo(root, &doc);

        let updated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: topic — gpt-5\n",
            "Recovered body.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, updated).unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", "test.md"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .status()
            .unwrap();

        snapshot::save(&doc, content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(content), Some(updated)).unwrap();
        crate::cycle_state::mark_write_applied(
            &doc,
            "write_template",
            Some(content),
            Some(updated),
        )
        .unwrap();

        let repaired = run(&doc).unwrap();
        assert_eq!(repaired, RepairOutcome::CommitBoundaryRecovered);

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
        assert_eq!(state.last_event, "repair_commit_boundary_recovered");
        assert_eq!(snapshot::load(&doc).unwrap().as_deref(), Some(updated));
    }

    #[test]
    fn recover_fails_closed_on_ambiguous_preflight_started_patchback_without_artifact() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let updated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: topic — gpt-5\n",
            "Recovered body.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, updated).unwrap();

        let err = run(&doc).unwrap_err();
        let message = err.to_string();
        assert!(message.contains(AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR));
        assert!(message.contains("### Re: topic — gpt-5"));
    }

    #[test]
    fn recover_ignores_committed_capture_without_pending() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\nbody\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        crate::capture::capture_response(&doc, "Recovered answer.").unwrap();
        crate::capture::mark_committed(&doc).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(
            recovered,
            RepairOutcome::Noop,
            "committed captures should not trigger replay/dedup on later preflights"
        );
    }

    #[test]
    fn recover_replays_latest_committed_capture_when_matching_prompt_was_left_orphaned() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "*Compacted.*\n\n",
            "❯ #code-review\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: code review — gpt-5\n\n",
            "Recovered body.\n",
            "<!-- /patch:exchange -->\n"
        );
        crate::capture::capture_response(&doc, response).unwrap();
        crate::capture::mark_committed(&doc).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("❯ #code-review"));
        assert!(result.contains("### Re: code review — gpt-5"));
        assert!(result.contains("Recovered body."));
    }

    #[test]
    fn recover_repairs_escaped_exchange_tail_when_response_already_present() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep\n",
            "<!-- /agent:pending -->\n\n",
            "## Assistant\n\n",
            "Recovered answer.\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        save_pending(&doc, "Recovered answer.").unwrap();
        let recovered = run(&doc).unwrap();
        assert_eq!(
            recovered,
            RepairOutcome::AlreadyApplied,
            "dedup path should skip replay"
        );

        let repaired = std::fs::read_to_string(&doc).unwrap();
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let assistant = repaired.find("## Assistant").unwrap();
        assert!(
            assistant < exchange_close,
            "escaped assistant block should move back inside exchange:\n{repaired}"
        );
    }

    #[test]
    fn recover_fails_closed_on_transcript_shaped_template_replay() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Hello\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let transcript_dump = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Hello\n",
            "### Re: topic — gpt-5\n",
            "Body\n",
            "<!-- agent:boundary:def456 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        save_pending(&doc, transcript_dump).unwrap();

        let err = run(&doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("refused to replay pending response"),
            "unexpected error: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            content,
            "blocked replay must not mutate the document"
        );

        let blocked_dir = dir.path().join(".agent-doc/repair-blocked");
        let captures: Vec<_> = std::fs::read_dir(&blocked_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .collect();
        assert_eq!(captures.len(), 1, "expected one blocked repair capture");
        let blocked_payload = std::fs::read_to_string(captures[0].path()).unwrap();
        assert!(blocked_payload.contains("agent component markers"));
        assert!(blocked_payload.contains("response_body"));
    }

    #[test]
    fn repair_crosses_commit_boundary_for_git_backed_replay() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        init_git_repo(dir.path(), &doc);

        save_pending(&doc, "This is the recovered response.").unwrap();

        let outcome = repair(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::ReplayedResponse);

        let head = crate::git::show_head(&doc).unwrap().unwrap();
        assert!(
            head.contains("This is the recovered response."),
            "HEAD should contain the recovered response:\n{head}"
        );

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);

        let capture = crate::capture::load_active(&doc).unwrap().unwrap();
        assert_eq!(capture.state, crate::capture::CaptureState::Committed);
        assert!(
            capture.replayed_at.is_some(),
            "recovered patchback should retain replay provenance"
        );
        assert!(
            capture.committed_at.is_some(),
            "recovered patchback should record the later commit boundary"
        );
    }

    #[test]
    fn repair_crosses_commit_boundary_when_response_already_present() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep\n",
            "<!-- /agent:pending -->\n\n",
            "## Assistant\n\n",
            "Recovered answer.\n"
        );
        std::fs::write(&doc, base).unwrap();
        snapshot::save(&doc, base).unwrap();
        init_git_repo(dir.path(), &doc);

        save_pending(&doc, "Recovered answer.").unwrap();

        let outcome = repair(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::AlreadyApplied);

        let head = crate::git::show_head(&doc).unwrap().unwrap();
        assert!(
            head.contains("Recovered answer."),
            "HEAD should contain the deduped recovered response:\n{head}"
        );
        let exchange_close = head.find("<!-- /agent:exchange -->").unwrap();
        let assistant = head.find("## Assistant").unwrap();
        assert!(
            assistant < exchange_close,
            "HEAD should keep the repaired assistant content inside exchange:\n{head}"
        );

        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("Recovered answer."),
            "snapshot should be advanced to the recovered response:\n{snap}"
        );

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
    }

    #[test]
    fn repair_commits_already_present_response_when_snapshot_lags_committed_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let base = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Why did the patchback miss?\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        snapshot::save(&doc, base).unwrap();
        init_git_repo(dir.path(), &doc);

        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(base), Some(base)).unwrap();

        let direct_patch = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Why did the patchback miss?\n",
            "### Re: missed patchback — gpt-5\n\n",
            "Recovered through direct patch.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, direct_patch).unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: missed patchback — gpt-5\n\n",
            "Recovered through direct patch.\n",
            "<!-- /patch:exchange -->\n"
        );
        save_pending(&doc, response).unwrap();

        let outcome = repair(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::AlreadyApplied);

        let head = crate::git::show_head(&doc).unwrap().unwrap();
        assert!(
            head.contains("Recovered through direct patch."),
            "HEAD should own the already-present response after repair:\n{head}"
        );

        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("Recovered through direct patch."),
            "snapshot should advance to the already-present response:\n{snap}"
        );

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
    }
}
