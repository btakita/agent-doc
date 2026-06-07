//! # Module: queue
//!
//! Pure functions for parsing and mutating the `agent:queue` component body.
//!
//! Hybrid syntax:
//! - `- text` → single-line prompt
//! - `~~~prompt` / `---` → multi-line prompt fence
//! - `--- start [at <datetime>]` / `~~~start` → start fence (activation signal)
//! - `--- stop` / `~~~stop` → stop fence (breakpoint)
//! - any other non-empty line (and an unclosed fence opener) → `Freeform`:
//!   preserved verbatim and never treated as an actionable prompt/dispatch
//!   target. `parse` is fully tolerant of a polluted queue (free-text / log
//!   dumps / stray `---` separators merged into the component by an earlier
//!   corruption) and never bails — consume/resume/dispatch guards stay
//!   resilient. A fence opener is only a fence when a matching closer follows;
//!   otherwise it is preserved as a `Freeform` separator and the lines beneath
//!   it (presets, real `do [#id]` items) still parse normally.
//!
//! Activation resolution (Phase 2):
//! - `resolve_activation()` determines whether the queue should be active
//!   based on (in priority order): `auto` attribute, inline start fence,
//!   exchange trigger (`do queue`/`run queue`), persisted `queue_active` state.
//!
//! Halt detection (Phase 3):
//! - `detect_head_prompt_modified()` compares the head prompt between snapshot
//!   and file to detect user edits between cycles.
//! - Stop fence at head → halt signal for preflight.
//!
//! This module is I/O-free. Callers handle reading/writing files.

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueEntry {
    Prompt(QueuePrompt),
    Completed(QueuePrompt),
    Preset(String),
    Dispatch(String),
    StartFence(Option<String>),
    StopFence,
    /// Unrecognized free-text line preserved verbatim. The queue body can be
    /// polluted by an earlier corruption that merged prose / log dumps into the
    /// `agent:queue` component (`#jb-run-agent-doc-response-queue-contamination`).
    /// Rather than fail every consume/resume/dispatch guard that parses the
    /// queue, such lines are captured as `Freeform`: rendered back verbatim and
    /// never treated as an actionable prompt/dispatch target.
    Freeform(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuePrompt {
    pub text: String,
    pub multiline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct QueueComponent {
    pub auto: bool,
    pub entries: Vec<QueueEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueTrigger {
    Auto,
    StartFence,
    ExchangeRequest,
    Persisted,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QueueActivation {
    pub active: bool,
    pub trigger: Option<QueueTrigger>,
    pub deferred: bool,
    pub start_at: Option<String>,
    pub consumed_start_fence: bool,
    pub entries_after: Vec<QueueEntry>,
}

/// Resolve whether the queue should be activated this cycle.
///
/// Priority order:
/// 1. `auto` attribute on `<!-- agent:queue auto -->` → immediate if prompts exist
/// 2. Start fence at head of entries → consume bare `--- start`; defer `--- start at <time>`
/// 3. Exchange trigger (`do queue` / `run queue` detected in diff)
/// 4. Persisted `queue_active: true` from frontmatter
///
/// Returns the resolved activation state including whether a start fence was consumed.
pub fn resolve_activation(
    entries: &[QueueEntry],
    has_auto: bool,
    exchange_triggered: bool,
    persisted_active: bool,
) -> QueueActivation {
    let has_prompts = !prompts(entries).is_empty();

    // Priority 1: auto attribute
    if has_auto && has_prompts {
        return QueueActivation {
            active: true,
            trigger: Some(QueueTrigger::Auto),
            entries_after: entries.to_vec(),
            ..Default::default()
        };
    }

    // Priority 2: start fence at head
    if let Some(QueueEntry::StartFence(datetime)) = entries.first() {
        if let Some(dt) = datetime {
            return QueueActivation {
                deferred: true,
                start_at: Some(dt.clone()),
                entries_after: entries.to_vec(),
                ..Default::default()
            };
        } else {
            let remaining: Vec<QueueEntry> = entries[1..].to_vec();
            let has_remaining_prompts = !prompts(&remaining).is_empty();
            return QueueActivation {
                active: has_remaining_prompts,
                trigger: if has_remaining_prompts {
                    Some(QueueTrigger::StartFence)
                } else {
                    None
                },
                consumed_start_fence: true,
                entries_after: remaining,
                ..Default::default()
            };
        }
    }

    // Priority 3: exchange trigger
    if exchange_triggered && has_prompts {
        return QueueActivation {
            active: true,
            trigger: Some(QueueTrigger::ExchangeRequest),
            entries_after: entries.to_vec(),
            ..Default::default()
        };
    }

    // Priority 4: persisted active state
    if persisted_active && has_prompts {
        return QueueActivation {
            active: true,
            trigger: Some(QueueTrigger::Persisted),
            entries_after: entries.to_vec(),
            ..Default::default()
        };
    }

    QueueActivation {
        entries_after: entries.to_vec(),
        ..Default::default()
    }
}

/// Reconstruct an `<!-- agent:queue -->` opening tag without the `auto` attribute.
pub fn strip_auto_from_tag(tag: &str) -> String {
    tag.replace(" auto", "")
}

pub fn parse(body: &str) -> Result<Vec<QueueEntry>> {
    let mut entries = Vec::new();
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        // A queue item is `- <text>`. Tolerate a single stray leading backtick
        // (`` `- text ``, a common mistype where the operator's code-span tick
        // landed before the bullet) by normalizing it to `- text`, so the item
        // parses — and re-renders — as a real prompt instead of being silently
        // preserved as inert `Freeform` and skipped (#queue-line-leading-backtick-drop).
        let item_line = match line.strip_prefix('`') {
            Some(rest) if rest.starts_with("- ") => rest,
            _ => line,
        };
        if let Some(rest) = item_line.strip_prefix("- ") {
            if let Some(completed) = parse_completed_inline(rest) {
                entries.push(QueueEntry::Completed(QueuePrompt {
                    text: completed.to_string(),
                    multiline: false,
                }));
            } else if is_reference_directive(rest) {
                // Optional-`do` grammar (Stage 1): a `re [#id]` / `re #id` line
                // *references* a tracked id without executing it. Preserve it
                // verbatim as `Freeform` so it is never run, synced, or reaped.
                entries.push(QueueEntry::Freeform(line.to_string()));
            } else {
                entries.push(QueueEntry::Prompt(QueuePrompt {
                    text: rest.to_string(),
                    multiline: false,
                }));
            }
            i += 1;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("preset ") {
            let preset = rest.trim();
            if !preset.is_empty() {
                entries.push(QueueEntry::Preset(preset.to_string()));
                i += 1;
                continue;
            }
        }

        if let Some(rest) = trimmed.strip_prefix("dispatch ") {
            let preset = rest.trim();
            if !preset.is_empty() {
                entries.push(QueueEntry::Dispatch(preset.to_string()));
                i += 1;
                continue;
            }
        }

        if is_start_fence(trimmed) {
            let datetime = parse_start_datetime(trimmed);
            entries.push(QueueEntry::StartFence(datetime));
            i += 1;
            continue;
        }

        if is_stop_fence(trimmed) {
            entries.push(QueueEntry::StopFence);
            i += 1;
            continue;
        }

        if is_completed_fence_open(trimmed) || is_prompt_fence_open(trimmed) {
            let completed = is_completed_fence_open(trimmed);
            let closer = fence_closer(trimmed);
            // Only treat the line as a fence-open when a matching closer
            // exists ahead. An unclosed fence is preserved as a `Freeform`
            // separator and the following lines parse normally, so a polluted
            // queue (stray ``` / ~~~ from merged prose) cannot swallow the real
            // queue items beneath it (#jb-run-agent-doc-response-queue-contamination).
            if let Some(close_idx) = (i + 1..lines.len()).find(|&j| lines[j].trim() == closer) {
                let text = lines[i + 1..close_idx].join("\n");
                if !text.trim().is_empty() {
                    let prompt = QueuePrompt {
                        text,
                        multiline: true,
                    };
                    if completed {
                        entries.push(QueueEntry::Completed(prompt));
                    } else {
                        entries.push(QueueEntry::Prompt(prompt));
                    }
                }
                i = close_idx + 1;
                continue;
            }
            entries.push(QueueEntry::Freeform(line.to_string()));
            i += 1;
            continue;
        }

        if is_bare_fence_open(trimmed) {
            // A bare `---` is a fence only when a closing `---` follows;
            // otherwise it is a stray separator preserved as `Freeform` so the
            // remaining lines (presets, real `do [#id]` items) still parse.
            if let Some(close_idx) = (i + 1..lines.len()).find(|&j| lines[j].trim() == "---") {
                let text = lines[i + 1..close_idx].join("\n");
                if !text.trim().is_empty() {
                    entries.push(QueueEntry::Prompt(QueuePrompt {
                        text,
                        multiline: true,
                    }));
                }
                i = close_idx + 1;
                continue;
            }
            entries.push(QueueEntry::Freeform(line.to_string()));
            i += 1;
            continue;
        }

        // Unrecognized line: preserve verbatim instead of failing the parse so
        // queue consume/resume/dispatch guards stay resilient to a polluted
        // queue body (#jb-run-agent-doc-response-queue-contamination). The line
        // is preserved as-is and never treated as an actionable item.
        entries.push(QueueEntry::Freeform(line.to_string()));
        i += 1;
    }

    Ok(entries)
}

pub fn render(entries: &[QueueEntry]) -> String {
    let mut out = String::new();
    for entry in entries {
        match entry {
            QueueEntry::Prompt(p) => {
                if p.multiline {
                    out.push_str("---\n");
                    out.push_str(&p.text);
                    if !p.text.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str("---\n");
                } else {
                    out.push_str("- ");
                    out.push_str(&p.text);
                    out.push('\n');
                }
            }
            QueueEntry::Completed(p) => {
                if p.multiline {
                    out.push_str("~~~done\n");
                    out.push_str(&p.text);
                    if !p.text.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str("~~~\n");
                } else {
                    out.push_str("- ~");
                    out.push_str(&p.text);
                    out.push_str("~\n");
                }
            }
            QueueEntry::Preset(preset) => {
                out.push_str("preset ");
                out.push_str(preset);
                out.push('\n');
            }
            QueueEntry::Dispatch(preset) => {
                out.push_str("dispatch ");
                out.push_str(preset);
                out.push('\n');
            }
            QueueEntry::StartFence(dt) => {
                if let Some(datetime) = dt {
                    out.push_str(&format!("--- start at {}\n", datetime));
                } else {
                    out.push_str("--- start\n");
                }
            }
            QueueEntry::StopFence => {
                out.push_str("--- stop\n");
            }
            QueueEntry::Freeform(line) => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// Backlog→queue sync mode parsed from the `queue` attribute on
/// `agent:backlog` / `agent:icebox` (`#backlog-queue-sync-attr`).
///
/// - `Sync` — the queue body is fully regenerated as the active-backlog
///   `do [#id]` list, in backlog order. Any other queue content (manual
///   presets, fences, struck items) is dropped. Use this when the queue should
///   mirror the backlog exactly each cycle.
/// - `Append` — add `do [#id]` for active backlog ids not already referenced in
///   the queue; existing entries and order are preserved. **Default** for a
///   bare `queue` attribute.
/// - `Prepend` — like `Append`, but the new prompts are inserted at the front
///   (in backlog order) instead of appended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BacklogQueueSyncMode {
    Sync,
    Append,
    Prepend,
}

impl BacklogQueueSyncMode {
    /// Parse the `queue` attribute value. An empty value (the bare `queue`
    /// token, e.g. `<!-- agent:backlog queue -->`) defaults to `Append`.
    /// Returns `None` for an unrecognized value.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "" | "append" => Some(Self::Append),
            "sync" => Some(Self::Sync),
            "prepend" => Some(Self::Prepend),
            _ => None,
        }
    }
}

/// Optional-`do` grammar (Stage 1): true when a queue line is a `re [#id]` /
/// `re #id` *reference* — it names a tracked id without executing it. Requires
/// the literal leading verb `re ` (so prose like `rebuild`, `re-run`, or
/// `reference …` is unaffected) immediately followed by a `#`/`[#` id token.
fn is_reference_directive(text: &str) -> bool {
    let lower = text.trim().to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("re ") else {
        return false;
    };
    let rest = rest.trim_start();
    rest.starts_with("[#") || rest.starts_with('#')
}

/// Extract the `#id` from `do [#id]` (or `do #id`) prompt text, normalized to
/// lowercase. Returns `None` for prompts that do not reference an id.
fn do_prompt_id(text: &str) -> Option<String> {
    let marker = text.find('#')?;
    let id: String = text[marker + 1..]
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect();
    if id.is_empty() {
        None
    } else {
        Some(id.to_ascii_lowercase())
    }
}

/// The backlog `#id` referenced by a queue entry's `do [#id]` text. Matches both
/// active (`Prompt`) and struck (`Completed`) entries so a synced item is not
/// re-added by `Append`/`Prepend` after it has been consumed.
fn entry_do_id(entry: &QueueEntry) -> Option<String> {
    match entry {
        QueueEntry::Prompt(p) | QueueEntry::Completed(p) => do_prompt_id(&p.text),
        _ => None,
    }
}

/// Build a single-line `do [#id]` queue prompt entry.
fn do_prompt_entry(id: &str) -> QueueEntry {
    QueueEntry::Prompt(QueuePrompt {
        text: format!("do [#{id}]"),
        multiline: false,
    })
}

/// Regenerate queue entries from active backlog ids per `mode`.
///
/// Returns `Some(new_entries)` when the queue changes and `None` when it already
/// matches (idempotent — safe to run on every preflight cycle). `backlog_ids`
/// are taken in backlog document order; duplicates and empties are dropped.
pub fn sync_backlog_into_queue(
    entries: &[QueueEntry],
    backlog_ids: &[String],
    mode: BacklogQueueSyncMode,
) -> Option<Vec<QueueEntry>> {
    // Normalize backlog ids: lowercase, drop empties, dedupe (first-seen order).
    let mut seen = std::collections::HashSet::new();
    let ordered_ids: Vec<String> = backlog_ids
        .iter()
        .map(|id| id.trim().to_ascii_lowercase())
        .filter(|id| !id.is_empty() && seen.insert(id.clone()))
        .collect();

    let existing_ids: std::collections::HashSet<String> =
        entries.iter().filter_map(entry_do_id).collect();

    let new_entries: Vec<QueueEntry> = match mode {
        BacklogQueueSyncMode::Sync => {
            // Full mirror: queue body becomes exactly the active-backlog do-list.
            ordered_ids.iter().map(|id| do_prompt_entry(id)).collect()
        }
        BacklogQueueSyncMode::Append => {
            let mut rebuilt = entries.to_vec();
            for id in &ordered_ids {
                if !existing_ids.contains(id) {
                    rebuilt.push(do_prompt_entry(id));
                }
            }
            rebuilt
        }
        BacklogQueueSyncMode::Prepend => {
            let missing: Vec<QueueEntry> = ordered_ids
                .iter()
                .filter(|id| !existing_ids.contains(*id))
                .map(|id| do_prompt_entry(id))
                .collect();
            if missing.is_empty() {
                entries.to_vec()
            } else {
                let mut rebuilt = missing;
                rebuilt.extend(entries.iter().cloned());
                rebuilt
            }
        }
    };

    if new_entries == entries {
        None
    } else {
        Some(new_entries)
    }
}

/// Stable-sort the `Prompt` entries of a queue by the priority `rank` of their
/// `do [#id]` id (`#backlog-priority-attribute`), preserving every non-prompt
/// entry (completed, preset, dispatch, fence, freeform) at its original
/// position. Ids absent from `rank` sort last (rank `u8::MAX`). Returns
/// `Some(new_entries)` when the order changes, `None` otherwise.
/// Two-tier manual-priority pin markers (`#queue-manual-priority-override`).
///
/// A queue / backlog / icebox item prefixed with a pin marker floats above the
/// `priority`-rank-ordered tail and is held there across the per-turn recompute
/// (it lives in the document text). Releasing a pin is just deleting the marker
/// — the item reverts to its `priority`-attribute rank. There are two tiers, so
/// operator priority always outranks agent priority:
///
/// - [`PRIORITIZED_MARKER`] `__prioritized__` (double underscore) — **operator**
///   pin. Top tier.
/// - [`AGENT_PRIORITIZED_MARKER`] `_prioritized_` (single underscore) — **agent**
///   pin, for items the agent prioritized. Middle tier: above unpinned items,
///   never above operator pins. (`#queue-agent-vs-operator-pin-tier`.)
pub const PRIORITIZED_MARKER: &str = "__prioritized__";
pub const AGENT_PRIORITIZED_MARKER: &str = "_prioritized_";

/// True when `text` carries the **operator** pin marker (`__prioritized__`) at
/// its head (after optional leading whitespace).
pub fn is_prioritized(text: &str) -> bool {
    text.trim_start().starts_with(PRIORITIZED_MARKER)
}

/// True when `text` carries the **agent** pin marker (`_prioritized_`) at its
/// head but is not an operator pin. `__prioritized__` does not start with
/// `_prioritized_` (second char differs: `_` vs `p`), so the two are cleanly
/// separable; the explicit operator-pin guard is belt-and-suspenders.
pub fn is_agent_prioritized(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with(AGENT_PRIORITIZED_MARKER) && !t.starts_with(PRIORITIZED_MARKER)
}

/// Pin tier of a prompt's text: `0` = operator pin, `1` = agent pin, `2` =
/// unpinned. Lower tiers sort first; agent pins never outrank operator pins.
fn priority_tier(text: &str) -> u8 {
    if is_prioritized(text) {
        0
    } else if is_agent_prioritized(text) {
        1
    } else {
        2
    }
}

fn entry_priority_tier(entry: &QueueEntry) -> u8 {
    match entry {
        QueueEntry::Prompt(p) | QueueEntry::Completed(p) => priority_tier(&p.text),
        _ => 2,
    }
}

pub fn sort_prompts_by_priority(
    entries: &[QueueEntry],
    rank: &std::collections::HashMap<String, u8>,
) -> Option<Vec<QueueEntry>> {
    let positions: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| matches!(e, QueueEntry::Prompt(_)).then_some(i))
        .collect();
    if positions.len() < 2 {
        return None;
    }
    // Sort key (tier, rank): operator pins (`__prioritized__`, tier 0) float to
    // the top, then agent pins (`_prioritized_`, tier 1), then the unpinned tail
    // (tier 2) in backlog-priority rank order. Pinned tiers use a constant
    // secondary key so the stable sort holds their document order. Deleting a
    // marker drops the prompt down a tier (ultimately into the rank-ordered tail).
    let key = |e: &QueueEntry| -> (u8, u8) {
        let tier = entry_priority_tier(e);
        if tier < 2 {
            (tier, 0)
        } else {
            let r = entry_do_id(e)
                .and_then(|id| rank.get(&id).copied())
                .unwrap_or(u8::MAX);
            (2, r)
        }
    };
    let mut prompts: Vec<QueueEntry> = positions.iter().map(|&i| entries[i].clone()).collect();
    let before: Vec<(u8, u8)> = prompts.iter().map(key).collect();
    prompts.sort_by_key(key);
    let after: Vec<(u8, u8)> = prompts.iter().map(key).collect();
    if before == after {
        return None;
    }
    let mut out = entries.to_vec();
    for (slot, &pos) in positions.iter().enumerate() {
        out[pos] = prompts[slot].clone();
    }
    Some(out)
}

fn parse_completed_inline(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    trimmed
        .strip_prefix("~~")
        .and_then(|s| s.strip_suffix("~~"))
        .or_else(|| trimmed.strip_prefix('~').and_then(|s| s.strip_suffix('~')))
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn is_start_fence(line: &str) -> bool {
    line == "--- start" || line.starts_with("--- start ") || line == "~~~start"
}

fn parse_start_datetime(line: &str) -> Option<String> {
    if line == "--- start" || line == "~~~start" {
        return None;
    }
    let rest = line.strip_prefix("--- start ")?;
    let rest = rest.strip_prefix("at ").unwrap_or(rest);
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn is_stop_fence(line: &str) -> bool {
    line == "--- stop" || line == "~~~stop"
}

fn is_prompt_fence_open(line: &str) -> bool {
    line == "~~~prompt"
}

fn is_completed_fence_open(line: &str) -> bool {
    line == "~~~done"
}

fn fence_closer(_open: &str) -> &'static str {
    "~~~"
}

fn is_bare_fence_open(line: &str) -> bool {
    line == "---" && !line.starts_with("--- ")
}

pub fn has_auto_attr(attrs: &std::collections::HashMap<String, String>) -> bool {
    attrs.contains_key("auto")
}

/// Detect a marker-side queue control (`start`/`go`/`stop`) on the
/// `<!-- agent:queue ... -->` opening tag (`#queue-state-unify`). These are the
/// marker spelling of the canonical `queue:` frontmatter control; preflight
/// migrates a present control into frontmatter and strips it from the tag, so
/// activation then flows through the normal persisted/frontmatter path. `auto`
/// is handled separately via [`has_auto_attr`] as the legacy alias. `stop` wins
/// over `start`/`go` if both are (erroneously) present.
pub fn marker_control(
    attrs: &std::collections::HashMap<String, String>,
) -> Option<agent_doc_core::frontmatter::QueueControl> {
    if attrs.contains_key("stop") {
        return Some(agent_doc_core::frontmatter::QueueControl::Stop);
    }
    if attrs.contains_key("start") || attrs.contains_key("go") {
        return Some(agent_doc_core::frontmatter::QueueControl::Start);
    }
    None
}

/// Reconstruct an `<!-- agent:queue -->` opening tag without any marker-side
/// control token (`start` / `go` / `stop`). Mirrors [`strip_auto_from_tag`].
pub fn strip_control_from_tag(tag: &str) -> String {
    tag.replace(" start", "")
        .replace(" go", "")
        .replace(" stop", "")
}

pub fn prompts(entries: &[QueueEntry]) -> Vec<&QueuePrompt> {
    entries
        .iter()
        .filter_map(|e| match e {
            QueueEntry::Prompt(p) => Some(p),
            _ => None,
        })
        .collect()
}

/// Collapse duplicate live `Prompt` entries that share the same trimmed text,
/// keeping the first occurrence. Two identical live queue prompts are never a
/// valid state — they only appear when a divergent IPC-buffer/snapshot CRDT or
/// 3-way merge duplicates a queue line (see `#adoc-queue-ipc-drift`). Returns
/// `None` when nothing changed so callers can avoid spurious mutations.
/// `Completed`/`Preset`/fence entries are left untouched.
pub fn dedup_live_prompts(entries: &[QueueEntry]) -> Option<Vec<QueueEntry>> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut deduped = Vec::with_capacity(entries.len());
    let mut dropped = false;
    for entry in entries {
        if let QueueEntry::Prompt(prompt) = entry
            && !seen.insert(prompt.text.trim().to_string())
        {
            dropped = true;
            continue;
        }
        deduped.push(entry.clone());
    }
    if dropped { Some(deduped) } else { None }
}

pub fn first_prompt(entries: &[QueueEntry]) -> Option<&QueuePrompt> {
    entries.iter().find_map(|e| match e {
        QueueEntry::Prompt(p) => Some(p),
        _ => None,
    })
}

#[allow(dead_code)]
pub fn remove_first_prompt(entries: &[QueueEntry]) -> Vec<QueueEntry> {
    let mut result = Vec::with_capacity(entries.len());
    let mut removed = false;
    for entry in entries {
        if !removed && matches!(entry, QueueEntry::Prompt(_)) {
            removed = true;
            continue;
        }
        result.push(entry.clone());
    }
    result
}

#[allow(dead_code)]
pub fn mark_first_prompt_completed(entries: &[QueueEntry]) -> Vec<QueueEntry> {
    mark_first_n_prompts_completed(entries, 1)
}

pub fn mark_first_n_prompts_completed(entries: &[QueueEntry], count: usize) -> Vec<QueueEntry> {
    let mut result = Vec::with_capacity(entries.len());
    let mut marked = 0usize;
    for entry in entries {
        if marked < count
            && let QueueEntry::Prompt(prompt) = entry
        {
            result.push(QueueEntry::Completed(prompt.clone()));
            marked += 1;
            continue;
        }
        result.push(entry.clone());
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum QueueHaltReason {
    StopFence,
    ItemModified,
}

/// Check if the head prompt has been modified between snapshot and file entries.
///
/// Compares the text of the first `Prompt` entry in each list. Returns `true`
/// if either list has no prompt or the texts differ.
///
/// This is the "stop-on-change" mechanism: if the user edits the next-to-consume
/// item between cycles, the queue halts immediately.
pub fn detect_head_prompt_modified(
    snapshot_entries: &[QueueEntry],
    file_entries: &[QueueEntry],
) -> bool {
    let snap_head = first_prompt(snapshot_entries);
    let file_head = first_prompt(file_entries);
    match (snap_head, file_head) {
        (Some(s), Some(f)) => {
            if s.text == f.text {
                return false;
            }
            // `#completed-queue-residue-regression` / `#queue-auto-no-continue`:
            // a NEW item inserted (or reordered) ahead of the still-present
            // in-flight head is a re-prioritization, not an in-place edit of the
            // head. Only treat the head as modified — and halt the queue — when
            // the snapshot head prompt is genuinely gone from the current queue
            // (edited in place or removed). Otherwise a concurrent prepend/reorder
            // would strand every remaining live prompt as inactive residue
            // instead of letting the auto-queue advance to the new head.
            let snap_head_still_present = file_entries
                .iter()
                .any(|entry| matches!(entry, QueueEntry::Prompt(p) if p.text == s.text));
            !snap_head_still_present
        }
        (None, None) => false,
        _ => true,
    }
}

/// Check if a stop fence is at the head of the entries (before any prompt).
pub fn has_stop_fence_at_head(entries: &[QueueEntry]) -> bool {
    matches!(
        first_live_control_or_prompt(entries),
        Some(QueueEntry::StopFence)
    )
}

/// Check if a time-gated start fence is at the head of the entries.
/// Returns `Some(datetime)` if a time gate is found.
pub fn time_gate_at_head(entries: &[QueueEntry]) -> Option<&str> {
    match first_live_control_or_prompt(entries) {
        Some(QueueEntry::StartFence(Some(dt))) => Some(dt.as_str()),
        _ => None,
    }
}

fn first_live_control_or_prompt(entries: &[QueueEntry]) -> Option<&QueueEntry> {
    entries.iter().find(|entry| {
        !matches!(
            entry,
            QueueEntry::Completed(_) | QueueEntry::Preset(_) | QueueEntry::Dispatch(_)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(value: &[&str]) -> Vec<String> {
        value.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reference_directive_excluded_from_prompts() {
        // Optional-`do` Stage 1: `re [#id]` / `re #id` references a tracked id
        // without executing it — never a runnable prompt, preserved verbatim.
        let entries = parse("- re [#opt]\n- re #opt2\n- do [#run]\n- [#bare]\n").unwrap();
        let prompt_texts: Vec<&str> = prompts(&entries).iter().map(|p| p.text.as_str()).collect();
        assert!(
            !prompt_texts.iter().any(|t| t.starts_with("re ")),
            "re-references must not be runnable prompts: {prompt_texts:?}"
        );
        // `do [#id]` and bare `[#id]` still execute.
        assert!(prompt_texts.contains(&"do [#run]"));
        assert!(prompt_texts.contains(&"[#bare]"));
        // The references round-trip verbatim as Freeform.
        let rendered = render(&entries);
        assert!(rendered.contains("- re [#opt]"));
        assert!(rendered.contains("- re #opt2"));
    }

    #[test]
    fn reference_directive_does_not_match_prose() {
        assert!(is_reference_directive("re [#opt]"));
        assert!(is_reference_directive("re #opt"));
        assert!(is_reference_directive("  re   [#opt] some note"));
        // Real words beginning with "re" are not references.
        assert!(!is_reference_directive("rebuild the index"));
        assert!(!is_reference_directive("re-run the tests"));
        assert!(!is_reference_directive("reference [#opt] in passing"));
        assert!(!is_reference_directive("re the meeting"));
        assert!(!is_reference_directive("do [#opt]"));
    }

    #[test]
    fn backlog_queue_sync_mode_parse() {
        use BacklogQueueSyncMode::*;
        assert_eq!(BacklogQueueSyncMode::parse(""), Some(Append));
        assert_eq!(BacklogQueueSyncMode::parse("append"), Some(Append));
        assert_eq!(BacklogQueueSyncMode::parse("sync"), Some(Sync));
        assert_eq!(BacklogQueueSyncMode::parse("prepend"), Some(Prepend));
        assert_eq!(BacklogQueueSyncMode::parse(" sync "), Some(Sync));
        assert_eq!(BacklogQueueSyncMode::parse("nope"), None);
    }

    #[test]
    fn sync_mode_fully_mirrors_backlog_order() {
        let entries = parse("- do [#old]\npreset spec\n").unwrap();
        let synced =
            sync_backlog_into_queue(&entries, &ids(&["a", "b"]), BacklogQueueSyncMode::Sync)
                .expect("queue should change");
        assert_eq!(render(&synced), "- do [#a]\n- do [#b]\n");
    }

    #[test]
    fn sync_mode_idempotent_when_already_mirrored() {
        let entries = parse("- do [#a]\n- do [#b]\n").unwrap();
        assert!(
            sync_backlog_into_queue(&entries, &ids(&["a", "b"]), BacklogQueueSyncMode::Sync)
                .is_none(),
            "already-mirrored queue must not re-mutate"
        );
    }

    #[test]
    fn append_mode_adds_only_missing_ids_at_tail() {
        let entries = parse("- do [#a]\n").unwrap();
        let synced = sync_backlog_into_queue(
            &entries,
            &ids(&["a", "b", "c"]),
            BacklogQueueSyncMode::Append,
        )
        .expect("queue should change");
        assert_eq!(render(&synced), "- do [#a]\n- do [#b]\n- do [#c]\n");
    }

    #[test]
    fn append_mode_skips_struck_completed_ids() {
        // A consumed/struck item must not be re-appended.
        let entries = parse("- ~do [#a]~\n").unwrap();
        assert!(
            sync_backlog_into_queue(&entries, &ids(&["a"]), BacklogQueueSyncMode::Append).is_none(),
            "struck id should count as present"
        );
    }

    #[test]
    fn prepend_mode_inserts_missing_ids_at_front_in_backlog_order() {
        let entries = parse("- do [#z]\n").unwrap();
        let synced = sync_backlog_into_queue(
            &entries,
            &ids(&["a", "b", "z"]),
            BacklogQueueSyncMode::Prepend,
        )
        .expect("queue should change");
        assert_eq!(render(&synced), "- do [#a]\n- do [#b]\n- do [#z]\n");
    }

    #[test]
    fn sync_dedupes_and_normalizes_case() {
        let entries: Vec<QueueEntry> = Vec::new();
        let synced =
            sync_backlog_into_queue(&entries, &ids(&["A", "a", "B"]), BacklogQueueSyncMode::Sync)
                .expect("queue should change");
        assert_eq!(render(&synced), "- do [#a]\n- do [#b]\n");
    }

    #[test]
    fn sort_prompts_by_priority_orders_do_prompts() {
        let entries = parse("- do [#a]\n- do [#b]\n- do [#c]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 3u8);
        rank.insert("b".to_string(), 1u8);
        rank.insert("c".to_string(), 2u8);
        let sorted = sort_prompts_by_priority(&entries, &rank).expect("order should change");
        assert_eq!(render(&sorted), "- do [#b]\n- do [#c]\n- do [#a]\n");
    }

    #[test]
    fn sort_prompts_by_priority_keeps_non_prompts_in_place() {
        let entries = parse("preset spec\n- do [#a]\n- do [#b]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 2u8);
        rank.insert("b".to_string(), 1u8);
        let sorted = sort_prompts_by_priority(&entries, &rank).expect("order should change");
        // preset stays at index 0; prompts reorder among themselves.
        assert_eq!(render(&sorted), "preset spec\n- do [#b]\n- do [#a]\n");
    }

    #[test]
    fn sort_prompts_by_priority_idempotent_when_ordered() {
        let entries = parse("- do [#a]\n- do [#b]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8);
        rank.insert("b".to_string(), 2u8);
        assert!(sort_prompts_by_priority(&entries, &rank).is_none());
    }

    #[test]
    fn prioritized_marker_floats_pinned_prompt_to_top() {
        // #queue-manual-priority-override: a `__prioritized__` pin floats above
        // higher-backlog-rank items regardless of its own rank.
        let entries = parse("- do [#a]\n- __prioritized__ do [#b]\n- do [#c]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8);
        rank.insert("b".to_string(), 9u8); // worst rank, but pinned → top
        rank.insert("c".to_string(), 2u8);
        let sorted = sort_prompts_by_priority(&entries, &rank).expect("pin should float");
        assert_eq!(
            render(&sorted),
            "- __prioritized__ do [#b]\n- do [#a]\n- do [#c]\n"
        );
    }

    #[test]
    fn prioritized_marker_floats_multiple_pins_in_document_order() {
        // Multiple pins keep their document order at the top; unpinned tail keeps rank order.
        let entries =
            parse("- do [#a]\n- __prioritized__ [#z]\n- do [#b]\n- __prioritized__ [#y]\n")
                .unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 2u8);
        rank.insert("b".to_string(), 1u8);
        let sorted = sort_prompts_by_priority(&entries, &rank).expect("pins should float");
        assert_eq!(
            render(&sorted),
            "- __prioritized__ [#z]\n- __prioritized__ [#y]\n- do [#b]\n- do [#a]\n"
        );
    }

    #[test]
    fn prioritized_marker_release_reverts_to_rank_order() {
        // Deleting the marker drops the item back into rank-ordered position.
        let entries = parse("- do [#a]\n- do [#b]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 2u8);
        rank.insert("b".to_string(), 1u8);
        let sorted = sort_prompts_by_priority(&entries, &rank).expect("rank reorders");
        assert_eq!(render(&sorted), "- do [#b]\n- do [#a]\n");
    }

    #[test]
    fn prioritized_marker_floats_pin_with_empty_rank_map() {
        // No backlog priority at all, but a pin must still float to the top.
        let entries = parse("- do [#a]\n- __prioritized__ do [#b]\n").unwrap();
        let rank = std::collections::HashMap::new();
        let sorted = sort_prompts_by_priority(&entries, &rank).expect("pin floats sans rank");
        assert_eq!(render(&sorted), "- __prioritized__ do [#b]\n- do [#a]\n");
    }

    #[test]
    fn is_prioritized_detects_marker() {
        assert!(is_prioritized("__prioritized__ do [#x]"));
        assert!(is_prioritized("  __prioritized__ [#x]"));
        assert!(!is_prioritized("do [#x]"));
        assert!(!is_prioritized("not __prioritized__ here"));
    }

    #[test]
    fn pin_tiers_operator_then_agent_then_unpinned() {
        // #queue-agent-vs-operator-pin-tier: operator pin (__) outranks agent pin (_)
        // outranks unpinned, regardless of backlog rank.
        let entries = parse(concat!(
            "- do [#a]\n",
            "- _prioritized_ do [#b]\n",
            "- __prioritized__ do [#c]\n",
        ))
        .unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8); // best rank, but unpinned → bottom
        rank.insert("b".to_string(), 5u8);
        rank.insert("c".to_string(), 9u8); // worst rank, but operator-pinned → top
        let sorted = sort_prompts_by_priority(&entries, &rank).expect("tiers reorder");
        assert_eq!(
            render(&sorted),
            "- __prioritized__ do [#c]\n- _prioritized_ do [#b]\n- do [#a]\n"
        );
    }

    #[test]
    fn is_agent_prioritized_distinguishes_single_from_double_underscore() {
        assert!(is_agent_prioritized("_prioritized_ do [#x]"));
        assert!(!is_agent_prioritized("__prioritized__ do [#x]")); // operator pin, not agent
        assert!(!is_prioritized("_prioritized_ do [#x]")); // agent pin is not operator pin
        assert!(!is_agent_prioritized("do [#x]"));
    }

    #[test]
    fn dedup_live_prompts_collapses_duplicate_live_prompt() {
        // #adoc-queue-ipc-drift: a merge-duplicated live head must collapse to one,
        // while Completed residue / Preset entries are preserved.
        let entries = parse(concat!(
            "preset #spec-test-build-install-commit-push\n",
            "- ~do [#adoc-sqlite-seam]~\n",
            "- do [#adoc-orch-shim-cleanup]\n",
            "- do [#adoc-orch-shim-cleanup]\n",
        ))
        .unwrap();
        let deduped = dedup_live_prompts(&entries).expect("duplicate should be collapsed");
        assert_eq!(
            prompts(&deduped).len(),
            1,
            "duplicate live prompt collapses to one: {deduped:?}"
        );
        assert_eq!(
            deduped
                .iter()
                .filter(|e| matches!(e, QueueEntry::Completed(_)))
                .count(),
            1
        );
        assert!(deduped.iter().any(|e| matches!(e, QueueEntry::Preset(_))));
    }

    #[test]
    fn dedup_live_prompts_noop_without_duplicates() {
        let entries = parse("- do [#a]\n- do [#b]\n").unwrap();
        assert!(
            dedup_live_prompts(&entries).is_none(),
            "no duplicates → no mutation"
        );
    }

    #[test]
    fn parse_single_line_items() {
        let body = "- do #fix1\n- do #fix2\n- run tests\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries[0],
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix1".to_string(),
                multiline: false,
            })
        );
        assert_eq!(
            entries[1],
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix2".to_string(),
                multiline: false,
            })
        );
        assert_eq!(
            entries[2],
            QueueEntry::Prompt(QueuePrompt {
                text: "run tests".to_string(),
                multiline: false,
            })
        );
    }

    #[test]
    fn parse_multiline_tilde_prompt() {
        let body = "~~~prompt\nReview the changes in src/.\nCheck for edge cases.\n~~~\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0],
            QueueEntry::Prompt(QueuePrompt {
                text: "Review the changes in src/.\nCheck for edge cases.".to_string(),
                multiline: true,
            })
        );
    }

    #[test]
    fn parse_multiline_dash_prompt() {
        let body = "---\nReview the changes.\nThen run cargo test.\n---\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0],
            QueueEntry::Prompt(QueuePrompt {
                text: "Review the changes.\nThen run cargo test.".to_string(),
                multiline: true,
            })
        );
    }

    #[test]
    fn parse_mixed_syntax() {
        let body = "- do #fix1\n---\nReview changes.\n---\n- run tests\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(matches!(&entries[0], QueueEntry::Prompt(p) if !p.multiline));
        assert!(matches!(&entries[1], QueueEntry::Prompt(p) if p.multiline));
        assert!(matches!(&entries[2], QueueEntry::Prompt(p) if !p.multiline));
    }

    #[test]
    fn parse_normalizes_stray_leading_backtick_on_item() {
        // #queue-line-leading-backtick-drop: a queue item mistyped with a stray
        // leading backtick (`` `- text ``, the operator's code-span tick landing
        // before the bullet) must parse as a real prompt — not be silently kept
        // as inert Freeform and skipped — and re-render canonically as `- text`.
        let body = "`- There is significant blocking with the sync pipeline.\n- run tests\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            QueueEntry::Prompt(QueuePrompt {
                text: "There is significant blocking with the sync pipeline.".to_string(),
                multiline: false,
            })
        );
        assert!(matches!(&entries[1], QueueEntry::Prompt(p) if p.text == "run tests"));
        // Re-render strips the stray backtick (self-heal to canonical `- `).
        let rendered = render(&entries);
        assert!(rendered.contains("- There is significant blocking"), "{rendered}");
        assert!(!rendered.contains("`-"), "{rendered}");
    }

    #[test]
    fn parse_completed_single_line_item() {
        let body = "- ~do #fix1~\n- do #fix2\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(matches!(&entries[0], QueueEntry::Completed(p) if p.text == "do #fix1"));
        assert!(matches!(&entries[1], QueueEntry::Prompt(p) if p.text == "do #fix2"));
        assert_eq!(prompts(&entries).len(), 1);
        assert_eq!(render(&entries), body);
    }

    #[test]
    fn stop_fence_after_completed_residue_is_live_head() {
        let body = "- ~do #fix1~\n--- stop\n- do #fix2\n";
        let entries = parse(body).unwrap();

        assert!(has_stop_fence_at_head(&entries));
    }

    #[test]
    fn time_gate_after_completed_residue_is_live_head() {
        let body = "- ~do #fix1~\n--- start at 17:00 ET\n- do #fix2\n";
        let entries = parse(body).unwrap();

        assert_eq!(time_gate_at_head(&entries), Some("17:00 ET"));
    }

    #[test]
    fn parse_preset_directive() {
        let body = "preset spec-test-build-install-commit-push\n- do #fix1\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            QueueEntry::Preset("spec-test-build-install-commit-push".to_string())
        );
        assert_eq!(prompts(&entries).len(), 1);
        assert_eq!(render(&entries), body);
    }

    #[test]
    fn parse_dispatch_directive() {
        let body = "dispatch #spec-test-build-install-commit-push\n- do #fix1\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            QueueEntry::Dispatch("#spec-test-build-install-commit-push".to_string())
        );
        assert_eq!(prompts(&entries).len(), 1);
        assert_eq!(render(&entries), body);
    }

    #[test]
    fn parse_empty_queue() {
        let body = "";
        let entries = parse(body).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_blank_lines_only() {
        let body = "\n\n\n";
        let entries = parse(body).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_start_fence_bare() {
        let body = "--- start\n- do #fix1\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], QueueEntry::StartFence(None));
    }

    #[test]
    fn parse_start_fence_with_time() {
        let body = "--- start at 17:00 ET\n- run nightly tests\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            QueueEntry::StartFence(Some("17:00 ET".to_string()))
        );
    }

    #[test]
    fn parse_start_fence_without_at() {
        let body = "--- start 17:00 ET\n- run tests\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            QueueEntry::StartFence(Some("17:00 ET".to_string()))
        );
    }

    #[test]
    fn parse_tilde_start_fence() {
        let body = "~~~start\n- do #fix1\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], QueueEntry::StartFence(None));
    }

    #[test]
    fn parse_stop_fence() {
        let body = "- do #fix1\n--- stop\n- do #fix2\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1], QueueEntry::StopFence);
    }

    #[test]
    fn parse_tilde_stop_fence() {
        let body = "- do #fix1\n~~~stop\n- do #fix2\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1], QueueEntry::StopFence);
    }

    #[test]
    fn parse_multiple_time_gates() {
        let body =
            "- do #fix1\n--- start 17:00 ET\n- run nightly\n--- start 18:00 ET\n- coverage\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 5);
        assert_eq!(
            entries[1],
            QueueEntry::StartFence(Some("17:00 ET".to_string()))
        );
        assert_eq!(
            entries[3],
            QueueEntry::StartFence(Some("18:00 ET".to_string()))
        );
    }

    #[test]
    fn parse_auto_attribute() {
        let mut attrs = std::collections::HashMap::new();
        attrs.insert("auto".to_string(), String::new());
        assert!(has_auto_attr(&attrs));

        let empty = std::collections::HashMap::new();
        assert!(!has_auto_attr(&empty));
    }

    #[test]
    fn parse_preserves_unexpected_content_as_freeform() {
        // Previously this bailed; now unrecognized lines are preserved as
        // non-actionable Freeform so a polluted queue cannot brick the
        // consume/resume/dispatch guards that parse it.
        let body = "random text that is not a list item or fence\n";
        let entries = parse(body).expect("unexpected content is tolerated as Freeform");
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0], QueueEntry::Freeform(_)));
        assert!(prompts(&entries).is_empty());
    }

    #[test]
    fn unclosed_prompt_fence_opener_preserved_as_freeform() {
        // Previously bailed; now an unclosed fence opener is preserved as a
        // Freeform separator so a polluted queue cannot brick the parse.
        let body = "~~~prompt\nSome content without closing fence\n";
        let entries = parse(body).expect("unclosed fence is tolerated");
        assert!(prompts(&entries).is_empty());
        assert!(entries.iter().any(|e| matches!(e, QueueEntry::Freeform(_))));
    }

    #[test]
    fn unclosed_dash_fence_opener_preserved_as_freeform() {
        let body = "---\nContent without closing dashes\n";
        let entries = parse(body).expect("unclosed --- fence is tolerated");
        assert!(prompts(&entries).is_empty());
        assert!(
            entries
                .iter()
                .any(|e| matches!(e, QueueEntry::Freeform(s) if s.trim() == "---"))
        );
    }

    #[test]
    fn render_single_line() {
        let entries = vec![QueueEntry::Prompt(QueuePrompt {
            text: "do #fix1".to_string(),
            multiline: false,
        })];
        assert_eq!(render(&entries), "- do #fix1\n");
    }

    #[test]
    fn render_multiline() {
        let entries = vec![QueueEntry::Prompt(QueuePrompt {
            text: "Review changes.\nRun tests.".to_string(),
            multiline: true,
        })];
        assert_eq!(render(&entries), "---\nReview changes.\nRun tests.\n---\n");
    }

    #[test]
    fn render_start_fence() {
        let entries = vec![QueueEntry::StartFence(None)];
        assert_eq!(render(&entries), "--- start\n");
    }

    #[test]
    fn render_start_fence_with_time() {
        let entries = vec![QueueEntry::StartFence(Some("17:00 ET".to_string()))];
        assert_eq!(render(&entries), "--- start at 17:00 ET\n");
    }

    #[test]
    fn render_stop_fence() {
        let entries = vec![QueueEntry::StopFence];
        assert_eq!(render(&entries), "--- stop\n");
    }

    #[test]
    fn render_mixed() {
        let entries = vec![
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix1".to_string(),
                multiline: false,
            }),
            QueueEntry::StopFence,
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix2".to_string(),
                multiline: false,
            }),
        ];
        assert_eq!(render(&entries), "- do #fix1\n--- stop\n- do #fix2\n");
    }

    #[test]
    fn mark_first_prompt_completed_preserves_later_prompts() {
        let entries = vec![
            QueueEntry::Preset("spec".to_string()),
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix1".to_string(),
                multiline: false,
            }),
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix2".to_string(),
                multiline: false,
            }),
        ];
        let result = mark_first_prompt_completed(&entries);
        assert_eq!(render(&result), "preset spec\n- ~do #fix1~\n- do #fix2\n");
        assert_eq!(prompts(&result).len(), 1);
    }

    #[test]
    fn mark_first_prompt_completed_preserves_dispatch_directive() {
        let entries = vec![
            QueueEntry::Dispatch("#spec".to_string()),
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix1".to_string(),
                multiline: false,
            }),
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix2".to_string(),
                multiline: false,
            }),
        ];
        let result = mark_first_prompt_completed(&entries);
        assert_eq!(
            render(&result),
            "dispatch #spec\n- ~do #fix1~\n- do #fix2\n"
        );
        assert_eq!(prompts(&result).len(), 1);
    }

    #[test]
    fn roundtrip_preserves_structure() {
        let body = "- do #fix1\n--- start at 17:00 ET\n- run nightly\n--- stop\n- do #fix2\n";
        let entries = parse(body).unwrap();
        let rendered = render(&entries);
        let reparsed = parse(&rendered).unwrap();
        assert_eq!(entries, reparsed);
    }

    #[test]
    fn prompts_filters_to_prompt_entries() {
        let entries = vec![
            QueueEntry::StartFence(None),
            QueueEntry::Prompt(QueuePrompt {
                text: "task1".to_string(),
                multiline: false,
            }),
            QueueEntry::StopFence,
            QueueEntry::Prompt(QueuePrompt {
                text: "task2".to_string(),
                multiline: false,
            }),
        ];
        let p = prompts(&entries);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].text, "task1");
        assert_eq!(p[1].text, "task2");
    }

    #[test]
    fn first_prompt_skips_control_fences() {
        let entries = vec![
            QueueEntry::StartFence(None),
            QueueEntry::Prompt(QueuePrompt {
                text: "task1".to_string(),
                multiline: false,
            }),
        ];
        assert_eq!(first_prompt(&entries).unwrap().text, "task1");
    }

    #[test]
    fn first_prompt_none_when_empty() {
        let entries: Vec<QueueEntry> = vec![];
        assert!(first_prompt(&entries).is_none());
    }

    #[test]
    fn remove_first_prompt_preserves_control_fences() {
        let entries = vec![
            QueueEntry::StartFence(None),
            QueueEntry::Prompt(QueuePrompt {
                text: "task1".to_string(),
                multiline: false,
            }),
            QueueEntry::Prompt(QueuePrompt {
                text: "task2".to_string(),
                multiline: false,
            }),
        ];
        let result = remove_first_prompt(&entries);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], QueueEntry::StartFence(None));
        assert!(matches!(&result[1], QueueEntry::Prompt(p) if p.text == "task2"));
    }

    #[test]
    fn empty_multiline_fence_is_skipped() {
        let body = "---\n\n---\n- do #fix1\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0],
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix1".to_string(),
                multiline: false,
            })
        );
    }

    #[test]
    fn blank_lines_between_items_ignored() {
        let body = "- do #fix1\n\n\n- do #fix2\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
    }

    // --- Activation resolution tests ---

    fn make_prompt(text: &str) -> QueueEntry {
        QueueEntry::Prompt(QueuePrompt {
            text: text.to_string(),
            multiline: false,
        })
    }

    // `#completed-queue-residue-regression` / `#queue-auto-no-continue`: a new
    // item inserted/reordered ahead of the still-present in-flight head is a
    // re-prioritization, not an in-place head edit, so it must NOT register as
    // `item_modified` (which would halt + strand the whole queue as residue).
    #[test]
    fn head_prompt_modified_false_when_new_item_inserted_ahead_of_present_head() {
        let snapshot = vec![make_prompt("do [#bbb]"), make_prompt("do [#ddd]")];
        let file = vec![
            make_prompt("do [#ccc]"),
            make_prompt("do [#bbb]"),
            make_prompt("do [#ddd]"),
        ];
        assert!(!detect_head_prompt_modified(&snapshot, &file));
    }

    #[test]
    fn head_prompt_modified_false_on_reorder_promoting_existing_item() {
        // Operator promoted #ddd above #bbb; #bbb is still present → reprioritize.
        let snapshot = vec![make_prompt("do [#bbb]"), make_prompt("do [#ddd]")];
        let file = vec![make_prompt("do [#ddd]"), make_prompt("do [#bbb]")];
        assert!(!detect_head_prompt_modified(&snapshot, &file));
    }

    #[test]
    fn head_prompt_modified_true_when_head_text_edited_in_place() {
        // The snapshot head text is gone from the queue (edited in place) → halt.
        let snapshot = vec![make_prompt("do [#bbb]"), make_prompt("do [#ddd]")];
        let file = vec![
            make_prompt("do [#bbb] with extra operator notes"),
            make_prompt("do [#ddd]"),
        ];
        assert!(detect_head_prompt_modified(&snapshot, &file));
    }

    #[test]
    fn head_prompt_modified_false_when_head_unchanged() {
        let snapshot = vec![make_prompt("do [#bbb]"), make_prompt("do [#ddd]")];
        let file = vec![make_prompt("do [#bbb]"), make_prompt("do [#ddd]")];
        assert!(!detect_head_prompt_modified(&snapshot, &file));
    }

    #[test]
    fn activation_auto_with_prompts() {
        let entries = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        let act = resolve_activation(&entries, true, false, false);
        assert!(act.active);
        assert_eq!(act.trigger, Some(QueueTrigger::Auto));
        assert!(!act.consumed_start_fence);
        assert_eq!(act.entries_after.len(), 2);
    }

    #[test]
    fn activation_auto_empty_queue() {
        let entries: Vec<QueueEntry> = vec![];
        let act = resolve_activation(&entries, true, false, false);
        assert!(!act.active);
        assert!(act.trigger.is_none());
    }

    #[test]
    fn activation_start_fence_bare() {
        let entries = vec![QueueEntry::StartFence(None), make_prompt("do #fix1")];
        let act = resolve_activation(&entries, false, false, false);
        assert!(act.active);
        assert_eq!(act.trigger, Some(QueueTrigger::StartFence));
        assert!(act.consumed_start_fence);
        assert_eq!(act.entries_after.len(), 1);
        assert!(matches!(&act.entries_after[0], QueueEntry::Prompt(p) if p.text == "do #fix1"));
    }

    #[test]
    fn activation_start_fence_bare_no_prompts_after() {
        let entries = vec![QueueEntry::StartFence(None)];
        let act = resolve_activation(&entries, false, false, false);
        assert!(!act.active);
        assert!(act.trigger.is_none());
        assert!(act.consumed_start_fence);
        assert!(act.entries_after.is_empty());
    }

    #[test]
    fn activation_start_fence_with_time_defers() {
        let entries = vec![
            QueueEntry::StartFence(Some("17:00 ET".to_string())),
            make_prompt("run nightly"),
        ];
        let act = resolve_activation(&entries, false, false, false);
        assert!(!act.active);
        assert!(act.deferred);
        assert_eq!(act.start_at, Some("17:00 ET".to_string()));
        assert!(!act.consumed_start_fence);
        assert_eq!(act.entries_after.len(), 2);
    }

    #[test]
    fn activation_exchange_trigger() {
        let entries = vec![make_prompt("do #fix1")];
        let act = resolve_activation(&entries, false, true, false);
        assert!(act.active);
        assert_eq!(act.trigger, Some(QueueTrigger::ExchangeRequest));
    }

    #[test]
    fn activation_exchange_trigger_empty_queue() {
        let entries: Vec<QueueEntry> = vec![];
        let act = resolve_activation(&entries, false, true, false);
        assert!(!act.active);
    }

    #[test]
    fn activation_persisted_active() {
        let entries = vec![make_prompt("do #fix1")];
        let act = resolve_activation(&entries, false, false, true);
        assert!(act.active);
        assert_eq!(act.trigger, Some(QueueTrigger::Persisted));
    }

    #[test]
    fn activation_persisted_empty_queue() {
        let entries: Vec<QueueEntry> = vec![];
        let act = resolve_activation(&entries, false, false, true);
        assert!(!act.active);
    }

    #[test]
    fn activation_canonical_queue_start_drives_persisted_active() {
        // End-to-end (#queue-state-unify): `queue: start` in frontmatter folds
        // onto `queue_active`, which feeds `resolve_activation` as
        // `persisted_active`, activating the queue with the Persisted trigger
        // (auto-loop-continuation eligible).
        let (fm, _) = agent_doc_core::frontmatter::parse(
            "---\nagent_doc_format: template\nqueue: start\n---\n\n",
        )
        .unwrap();
        let persisted_active = fm.queue_active.unwrap_or(false);
        assert!(persisted_active, "queue: start must set queue_active");

        let entries = vec![make_prompt("do #fix1")];
        let act = resolve_activation(&entries, false, false, persisted_active);
        assert!(act.active);
        assert_eq!(act.trigger, Some(QueueTrigger::Persisted));
    }

    #[test]
    fn marker_control_detects_start_go_stop() {
        use agent_doc_core::frontmatter::QueueControl;
        let mut attrs = std::collections::HashMap::new();
        assert_eq!(marker_control(&attrs), None);
        attrs.insert("go".to_string(), String::new());
        assert_eq!(marker_control(&attrs), Some(QueueControl::Start));
        attrs.clear();
        attrs.insert("start".to_string(), String::new());
        assert_eq!(marker_control(&attrs), Some(QueueControl::Start));
        attrs.clear();
        attrs.insert("stop".to_string(), String::new());
        assert_eq!(marker_control(&attrs), Some(QueueControl::Stop));
        // stop wins over start/go if both are present.
        attrs.insert("go".to_string(), String::new());
        assert_eq!(marker_control(&attrs), Some(QueueControl::Stop));
    }

    #[test]
    fn strip_control_from_tag_removes_control_tokens() {
        assert_eq!(
            strip_control_from_tag("<!-- agent:queue preset=\"#p\" go -->"),
            "<!-- agent:queue preset=\"#p\" -->"
        );
        assert_eq!(
            strip_control_from_tag("<!-- agent:queue start -->"),
            "<!-- agent:queue -->"
        );
        assert_eq!(
            strip_control_from_tag("<!-- agent:queue stop -->"),
            "<!-- agent:queue -->"
        );
        // No control token → unchanged.
        assert_eq!(
            strip_control_from_tag("<!-- agent:queue auto -->"),
            "<!-- agent:queue auto -->"
        );
    }

    #[test]
    fn activation_canonical_queue_stop_deactivates() {
        let (fm, _) =
            agent_doc_core::frontmatter::parse("---\nqueue: stop\nqueue_active: true\n---\n\n")
                .unwrap();
        // Canonical `queue: stop` wins over a stale `queue_active: true`.
        assert_eq!(fm.queue_active, Some(false));
        let entries = vec![make_prompt("do #fix1")];
        let act = resolve_activation(&entries, false, false, fm.queue_active.unwrap_or(false));
        assert!(!act.active);
    }

    #[test]
    fn activation_auto_takes_precedence_over_exchange() {
        let entries = vec![make_prompt("task")];
        let act = resolve_activation(&entries, true, true, false);
        assert_eq!(act.trigger, Some(QueueTrigger::Auto));
    }

    #[test]
    fn activation_start_fence_takes_precedence_over_exchange() {
        let entries = vec![QueueEntry::StartFence(None), make_prompt("task")];
        let act = resolve_activation(&entries, false, true, false);
        assert_eq!(act.trigger, Some(QueueTrigger::StartFence));
        assert!(act.consumed_start_fence);
    }

    #[test]
    fn activation_none_when_no_triggers() {
        let entries = vec![make_prompt("task")];
        let act = resolve_activation(&entries, false, false, false);
        assert!(!act.active);
        assert!(act.trigger.is_none());
    }

    #[test]
    fn strip_auto_from_tag_removes_auto() {
        assert_eq!(
            strip_auto_from_tag("<!-- agent:queue auto -->"),
            "<!-- agent:queue -->"
        );
        assert_eq!(
            strip_auto_from_tag("<!-- agent:queue auto patch=append -->"),
            "<!-- agent:queue patch=append -->"
        );
    }

    #[test]
    fn strip_auto_from_tag_noop_without_auto() {
        assert_eq!(
            strip_auto_from_tag("<!-- agent:queue -->"),
            "<!-- agent:queue -->"
        );
    }

    // --- Phase 3: halt detection tests ---

    #[test]
    fn detect_head_modified_same_prompt() {
        let snap = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        let file = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        assert!(!detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_different_prompt() {
        let snap = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        let file = vec![make_prompt("do #fix1 EDITED"), make_prompt("do #fix2")];
        assert!(detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_prompt_removed() {
        let snap = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        let file = vec![make_prompt("do #fix2")];
        assert!(detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_both_empty() {
        let snap: Vec<QueueEntry> = vec![];
        let file: Vec<QueueEntry> = vec![];
        assert!(!detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_snap_empty_file_has() {
        let snap: Vec<QueueEntry> = vec![];
        let file = vec![make_prompt("new item")];
        assert!(detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_ignores_later_changes() {
        let snap = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        let file = vec![make_prompt("do #fix1"), make_prompt("do #fix2 EDITED")];
        assert!(!detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_skips_control_fences() {
        let snap = vec![QueueEntry::StopFence, make_prompt("task")];
        let file = vec![QueueEntry::StopFence, make_prompt("task")];
        assert!(!detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn stop_fence_at_head_detected() {
        let entries = vec![QueueEntry::StopFence, make_prompt("task")];
        assert!(has_stop_fence_at_head(&entries));
    }

    #[test]
    fn stop_fence_not_at_head() {
        let entries = vec![make_prompt("task"), QueueEntry::StopFence];
        assert!(!has_stop_fence_at_head(&entries));
    }

    #[test]
    fn stop_fence_empty_entries() {
        let entries: Vec<QueueEntry> = vec![];
        assert!(!has_stop_fence_at_head(&entries));
    }

    #[test]
    fn time_gate_at_head_detected() {
        let entries = vec![
            QueueEntry::StartFence(Some("17:00 ET".to_string())),
            make_prompt("task"),
        ];
        assert_eq!(time_gate_at_head(&entries), Some("17:00 ET"));
    }

    #[test]
    fn time_gate_at_head_bare_start_not_time_gate() {
        let entries = vec![QueueEntry::StartFence(None), make_prompt("task")];
        assert_eq!(time_gate_at_head(&entries), None);
    }

    #[test]
    fn time_gate_at_head_prompt_first() {
        let entries = vec![
            make_prompt("task"),
            QueueEntry::StartFence(Some("17:00 ET".to_string())),
        ];
        assert_eq!(time_gate_at_head(&entries), None);
    }

    #[test]
    fn parse_preserves_unrecognized_freetext_as_freeform_instead_of_failing() {
        // A queue polluted with prose (the contamination class) must not fail
        // every consume/resume guard that parses the queue.
        let body = "JB `Run Agent Doc` error:\n- do [#existing]\nThe response should contain the prompt.\n";
        let entries = parse(body).expect("polluted queue must parse, not bail");
        // The free-text lines are preserved as Freeform.
        assert_eq!(
            entries
                .iter()
                .filter(|e| matches!(e, QueueEntry::Freeform(_)))
                .count(),
            2
        );
        // The real prompt item is still recognized and actionable.
        let prompts = prompts(&entries);
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].text, "do [#existing]");
    }

    #[test]
    fn freeform_round_trips_through_render() {
        let body = "stray prose line\n- do [#real]\n";
        let rendered = render(&parse(body).unwrap());
        assert!(rendered.contains("stray prose line"));
        assert!(rendered.contains("- do [#real]"));
        // Re-parsing the rendered output is stable.
        let reparsed = parse(&rendered).unwrap();
        assert_eq!(prompts(&reparsed).len(), 1);
        assert_eq!(
            reparsed
                .iter()
                .filter(|e| matches!(e, QueueEntry::Freeform(_)))
                .count(),
            1
        );
    }

    #[test]
    fn freeform_is_not_an_actionable_prompt() {
        let entries = vec![
            QueueEntry::Freeform("just a note".to_string()),
            QueueEntry::Freeform("another note".to_string()),
        ];
        assert!(prompts(&entries).is_empty());
        assert!(first_prompt(&entries).is_none());
    }

    #[test]
    fn unclosed_bare_fence_does_not_swallow_real_items_beneath_it() {
        // The exact live-corruption shape: a stray `---` separator with no
        // matching close, followed by the real preset + queue items. The `---`
        // must be preserved as Freeform and the items must still parse as
        // actionable prompts (not swallowed into an unclosed fence).
        let body = "---\npreset #spec-test\n- do [#first]\n- do [#second]\n";
        let entries = parse(body).expect("unbalanced fences must not fail the parse");
        let prompts = prompts(&entries);
        assert_eq!(prompts.len(), 2);
        assert_eq!(prompts[0].text, "do [#first]");
        assert_eq!(prompts[1].text, "do [#second]");
        assert!(
            entries
                .iter()
                .any(|e| matches!(e, QueueEntry::Preset(p) if p == "#spec-test"))
        );
        assert!(
            entries
                .iter()
                .any(|e| matches!(e, QueueEntry::Freeform(s) if s.trim() == "---")),
            "the stray --- separator is preserved as Freeform"
        );
    }

    #[test]
    fn unclosed_prompt_fence_is_preserved_as_freeform_separator() {
        // A stray ``` opener with no closer is preserved, and the `- do` item
        // beneath it still parses.
        let body = "```\n- do [#real]\n";
        let entries = parse(body).expect("unclosed prompt fence must not fail the parse");
        assert_eq!(prompts(&entries).len(), 1);
        assert_eq!(prompts(&entries)[0].text, "do [#real]");
    }
}
