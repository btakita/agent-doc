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
    rewrite_queue_tag_attrs(tag, |token| token != "auto")
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

        if crate::queue_command::is_slash_command(trimmed) {
            entries.push(QueueEntry::Prompt(QueuePrompt {
                text: trimmed.to_string(),
                multiline: false,
            }));
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
                } else if crate::queue_command::is_slash_command(&p.text) {
                    out.push_str(p.text.trim());
                    out.push('\n');
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
                    // Double-tilde markdown strikethrough so a completed queue
                    // item renders struck-through in the editor (#queue-strike-on-complete).
                    // The parser (`parse_completed_inline`) reads back both `~x~`
                    // and `~~x~~`, so legacy single-tilde residue still resolves.
                    out.push_str("- ~~");
                    out.push_str(&p.text);
                    out.push_str("~~\n");
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
/// Releasing a pin is just deleting the marker. There are two tiers with
/// different effects on the per-turn `priority` recompute:
///
/// - **Operator** pin (`#queue-operator-pin-position-lock`) is *position-locked*:
///   the `priority` attribute never moves it. It stays at the exact slot where
///   the operator placed it (the marker lives in the document text, so it is
///   sticky), and the unpinned / agent-pinned prompts reorder *around* it.
/// - **Agent** pin floats above the unpinned `priority`-rank-ordered tail (but
///   never above an operator pin) among the slots not held by an operator pin.
///
/// The marker is a markdown-emphasis wrap of the word `prioritized`, so it
/// renders distinctly in the editor and is released by deleting it:
///
/// - **Operator** pin (top tier) — any of: markdown **strong** emphasis on
///   `pin`/`prioritized` (`**pin**`, `__pin__`, `**prioritized**`,
///   `__prioritized__`); the emoji shortcodes `:pin:` / `:pushpin:`; or the
///   literal 📌 emoji.
/// - **Agent** pin (middle tier, above unpinned, never above operator pins;
///   `#queue-agent-vs-operator-pin-tier`) — any of: markdown *emphasis* (italic)
///   on `pin`/`prioritized` (`*pin*`, `_pin_`, `*prioritized*`, `_prioritized_`);
///   the `:round_pushpin:` shortcode; or the literal 📍 emoji.
///
/// Strong emphasis / pushpin == operator; italic emphasis / round-pushpin ==
/// agent. Both asterisk and underscore spellings are accepted (markdown treats
/// them identically), so toggling spelling never changes the tier.
pub const PRIORITIZED_MARKERS: [&str; 7] = [
    "**prioritized**",
    "__prioritized__",
    "**pin**",
    "__pin__",
    ":pushpin:",
    ":pin:",
    "📌",
];
pub const AGENT_PRIORITIZED_MARKERS: [&str; 6] = [
    "*prioritized*",
    "_prioritized_",
    "*pin*",
    "_pin_",
    ":round_pushpin:",
    "📍",
];
/// Canonical single-spelling constants (emoji-shortcode form).
pub const PRIORITIZED_MARKER: &str = ":pushpin:";
pub const AGENT_PRIORITIZED_MARKER: &str = ":round_pushpin:";

/// True when `text` carries an **operator** (strong-emphasis) pin marker at its
/// head (after optional leading whitespace), in either spelling.
pub fn is_prioritized(text: &str) -> bool {
    let t = text.trim_start();
    PRIORITIZED_MARKERS.iter().any(|m| t.starts_with(m))
}

/// True when `text` carries an **agent** (italic-emphasis) pin marker at its
/// head but is not an operator pin. A strong-emphasis wrap (`**x**` / `__x__`)
/// never starts with its italic counterpart (`*x*` / `_x_`) — second char
/// differs — so the tiers are cleanly separable; the operator-pin guard is
/// belt-and-suspenders.
pub fn is_agent_prioritized(text: &str) -> bool {
    let t = text.trim_start();
    !is_prioritized(text) && AGENT_PRIORITIZED_MARKERS.iter().any(|m| t.starts_with(m))
}

/// Strip leading operator/agent pin markers (any spelling) plus surrounding
/// whitespace from a prompt's text, returning the pin-independent content.
///
/// Used to compare two queue prompts for **identity** without being fooled by a
/// `:pushpin:` / `:round_pushpin:` annotation present on one side only
/// (`#queue-consume-pushpin-normalization`): the snapshot can hold the unpinned
/// spelling of a head while the live document holds the pinned spelling of the
/// same logical item. The pin is cosmetic priority metadata, not item identity,
/// so a head-equality check must normalize it away or it errors out the cycle
/// (`queue consume: snapshot head prompts ... do not match document head prompts`).
/// Repeated leading markers are stripped so a doubly-annotated head still
/// normalizes to its bare content.
pub fn strip_priority_markers(text: &str) -> String {
    let mut t = text.trim();
    loop {
        let trimmed = t.trim_start();
        let stripped = PRIORITIZED_MARKERS
            .iter()
            .chain(AGENT_PRIORITIZED_MARKERS.iter())
            .find_map(|m| trimmed.strip_prefix(m));
        match stripped {
            Some(rest) => t = rest.trim_start(),
            None => break,
        }
    }
    t.trim().to_string()
}

/// Prefix auto-promoted queue prompts with the canonical agent-priority marker.
///
/// Sorting by backlog priority / auto-DAG is binary-owned priority, not an
/// operator pin. When a prompt moves earlier during that sort, annotate it with
/// `:round_pushpin:` so the visible queue explains why it jumped. Existing
/// operator/agent markers are preserved.
pub fn annotate_agent_priority_promotions(
    before: &[QueueEntry],
    after: &[QueueEntry],
) -> Option<Vec<QueueEntry>> {
    let before_prompts: Vec<String> = before
        .iter()
        .filter_map(|entry| match entry {
            QueueEntry::Prompt(prompt) => Some(strip_priority_markers(&prompt.text)),
            _ => None,
        })
        .collect();
    if before_prompts.is_empty() {
        return None;
    }

    let mut used = vec![false; before_prompts.len()];
    let mut prompt_slot = 0usize;
    let mut changed = false;
    let mut out = after.to_vec();

    for entry in &mut out {
        let QueueEntry::Prompt(prompt) = entry else {
            continue;
        };
        let identity = strip_priority_markers(&prompt.text);
        let original_slot =
            before_prompts
                .iter()
                .enumerate()
                .find_map(|(slot, before_identity)| {
                    (!used[slot] && before_identity == &identity).then_some(slot)
                });
        if let Some(slot) = original_slot {
            used[slot] = true;
            if slot > prompt_slot
                && !is_prioritized(&prompt.text)
                && !is_agent_prioritized(&prompt.text)
            {
                prompt.text = format!("{} {}", AGENT_PRIORITIZED_MARKER, prompt.text.trim_start());
                changed = true;
            }
        }
        prompt_slot += 1;
    }

    changed.then_some(out)
}

/// Prefix operator-moved queue prompts with the canonical operator-priority marker.
///
/// A manually reordered priority queue should not be undone by the next
/// binary-owned priority recompute. When an existing prompt appears earlier in
/// the live queue than it did in the snapshot, annotate it with `:pushpin:` so
/// the authored priority is sticky. New prompts and prompts that only moved
/// later are ignored; existing operator pins are preserved.
pub fn annotate_operator_priority_reorders(
    snapshot: &[QueueEntry],
    current: &[QueueEntry],
) -> Option<Vec<QueueEntry>> {
    let snapshot_prompts: Vec<String> = snapshot
        .iter()
        .filter_map(|entry| match entry {
            QueueEntry::Prompt(prompt) => Some(strip_priority_markers(&prompt.text)),
            _ => None,
        })
        .collect();
    if snapshot_prompts.is_empty() {
        return None;
    }

    let mut used = vec![false; snapshot_prompts.len()];
    let mut prompt_slot = 0usize;
    let mut changed = false;
    let mut out = current.to_vec();

    for entry in &mut out {
        let QueueEntry::Prompt(prompt) = entry else {
            continue;
        };
        let identity = strip_priority_markers(&prompt.text);
        let original_slot =
            snapshot_prompts
                .iter()
                .enumerate()
                .find_map(|(slot, snapshot_identity)| {
                    (!used[slot] && snapshot_identity == &identity).then_some(slot)
                });
        if let Some(slot) = original_slot {
            used[slot] = true;
            if slot > prompt_slot && !is_prioritized(&prompt.text) {
                prompt.text = format!("{} {}", PRIORITIZED_MARKER, prompt.text.trim_start());
                changed = true;
            }
        }
        prompt_slot += 1;
    }

    changed.then_some(out)
}

/// Auto-pin freshly operator-added queue prompts with the operator priority
/// marker (`#7r2s`).
///
/// `annotate_operator_priority_reorders` only pins an *existing* prompt that
/// moved earlier; a brand-new line the operator just typed into the queue is
/// ignored, so the subsequent backlog-priority recompute can sort it below
/// `queue`-attr backlog items and silently deprioritize the line the operator
/// just placed. Treat a prompt present in `current` but absent from `snapshot`
/// (by pin-independent identity) whose `do [#id]` id is NOT one the binary just
/// appended from the backlog this cycle (`synced_ids`) as an operator-authored
/// addition, and prefix it with `:pushpin:` so it is position-locked at its
/// authored slot. Binary-synced backlog entries and already-pinned prompts are
/// left untouched.
pub fn annotate_manual_queue_additions(
    snapshot: &[QueueEntry],
    current: &[QueueEntry],
    synced_ids: &std::collections::HashSet<String>,
) -> Option<Vec<QueueEntry>> {
    let snapshot_identities: std::collections::HashSet<String> = snapshot
        .iter()
        .filter_map(|entry| match entry {
            QueueEntry::Prompt(prompt) => Some(strip_priority_markers(&prompt.text)),
            _ => None,
        })
        .collect();
    let synced_lc: std::collections::HashSet<String> = synced_ids
        .iter()
        .map(|id| id.to_ascii_lowercase())
        .collect();
    let mut changed = false;
    let mut out = current.to_vec();
    for entry in &mut out {
        let do_id = entry_do_id(entry);
        let QueueEntry::Prompt(prompt) = entry else {
            continue;
        };
        let identity = strip_priority_markers(&prompt.text);
        if snapshot_identities.contains(&identity) {
            continue; // not new this cycle
        }
        if do_id
            .as_deref()
            .is_some_and(|id| synced_lc.contains(&id.to_ascii_lowercase()))
        {
            continue; // binary appended it from the backlog, not an operator add
        }
        if is_prioritized(&prompt.text) || is_agent_prioritized(&prompt.text) {
            continue; // operator/agent already pinned it
        }
        prompt.text = format!("{} {}", PRIORITIZED_MARKER, prompt.text.trim_start());
        changed = true;
    }
    changed.then_some(out)
}

/// Pin tier of a prompt's text: `0` = operator pin, `1` = agent pin, `2` =
/// unpinned. Tier 0 (operator pin) is position-locked by
/// `sort_prompts_by_priority` (`#queue-operator-pin-position-lock`); among the
/// reorderable remainder, agent pins (tier 1) sort before unpinned (tier 2).
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
    let prompts: Vec<QueueEntry> = positions.iter().map(|&i| entries[i].clone()).collect();
    let n = prompts.len();
    // Operator pins (`__prioritized__` / `:pushpin:`, tier 0) are POSITION-LOCKED
    // (`#queue-operator-pin-position-lock`): the `priority` attribute must never
    // move an operator-pinned prompt — it stays at the exact slot where the
    // operator placed it. Only agent pins (`_prioritized_`, tier 1) and the
    // unpinned tail (tier 2) are reordered, and they fill *only the slots not
    // held by an operator pin*, agent pins first then unpinned in backlog rank
    // order. A constant secondary key for agent pins keeps their document order.
    let key = |idx: usize| -> (u8, u8) {
        let e = &prompts[idx];
        let tier = entry_priority_tier(e); // 1 (agent pin) or 2 (unpinned) here
        if tier < 2 {
            (tier, 0)
        } else {
            let r = entry_do_id(e)
                .and_then(|id| rank.get(&id).copied())
                .unwrap_or(u8::MAX);
            (2, r)
        }
    };
    let mut movable: Vec<usize> = (0..n)
        .filter(|&i| entry_priority_tier(&prompts[i]) != 0)
        .collect();
    movable.sort_by_key(|&i| key(i));
    // Reassemble: walk the prompt slots in document order. An operator-pinned
    // slot keeps its own prompt (anchored); every other slot draws the next
    // entry from the reordered `movable` queue.
    let mut mv = movable.into_iter();
    let mut order: Vec<usize> = Vec::with_capacity(n);
    for (i, prompt) in prompts.iter().enumerate() {
        if entry_priority_tier(prompt) == 0 {
            order.push(i);
        } else {
            order.push(
                mv.next()
                    .expect("movable slot count matches non-operator-pin prompts"),
            );
        }
    }
    if order.iter().enumerate().all(|(slot, &i)| slot == i) {
        return None;
    }
    let mut out = entries.to_vec();
    for (slot, &pos) in positions.iter().enumerate() {
        out[pos] = prompts[order[slot]].clone();
    }
    Some(out)
}

/// Priority-weighted topological sort of queue prompts (`#queue-auto-dag-priority`).
///
/// `deps` maps an item id to the ids it must be ordered *after* (`after=#id`,
/// from a backlog item's tokens); inline `after=` tokens on a queue prompt's own
/// text are merged in. Ordering is Kahn's algorithm with a priority-ordered ready
/// set: among prompts whose dependencies are already emitted, the next is chosen
/// by the same tie-break as [`sort_prompts_by_priority`] — operator pin, then
/// agent pin, then backlog-priority rank, then document order. Dependencies
/// therefore always precede their dependents, so a blocker outranks a pin (the
/// operator's stated exception), while an edge-free queue comes out identical to
/// the plain pin+priority sort. A dependency cycle is broken by emitting the
/// remaining prompts in priority order (never dropped). Returns `None` when there
/// are no resolvable edges (caller falls back to [`sort_prompts_by_priority`]) or
/// when the prompts are already in the computed order (idempotent).
pub fn sort_prompts_by_dag(
    entries: &[QueueEntry],
    rank: &std::collections::HashMap<String, u8>,
    deps: &std::collections::HashMap<String, Vec<String>>,
) -> Option<Vec<QueueEntry>> {
    let positions: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| matches!(e, QueueEntry::Prompt(_)).then_some(i))
        .collect();
    if positions.len() < 2 {
        return None;
    }
    let prompts: Vec<QueueEntry> = positions.iter().map(|&i| entries[i].clone()).collect();
    let n = prompts.len();

    // id -> prompt slot (first occurrence wins).
    let mut id_to_idx: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (i, e) in prompts.iter().enumerate() {
        if let Some(id) = entry_do_id(e) {
            id_to_idx.entry(id).or_insert(i);
        }
    }

    // prereq[i] = prompt slots that must precede slot i (resolvable present nodes).
    let mut prereq: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut any_edge = false;
    for (i, e) in prompts.iter().enumerate() {
        let mut dep_ids: Vec<String> = Vec::new();
        if let Some(id) = entry_do_id(e)
            && let Some(d) = deps.get(&id)
        {
            dep_ids.extend(d.iter().cloned());
        }
        if let QueueEntry::Prompt(p) | QueueEntry::Completed(p) = e {
            dep_ids.extend(agent_doc_core::pending::item_after_deps(&p.text));
        }
        for dep in dep_ids {
            if let Some(&j) = id_to_idx.get(&dep)
                && j != i
                && !prereq[i].contains(&j)
            {
                prereq[i].push(j);
                any_edge = true;
            }
        }
    }
    if !any_edge {
        return None;
    }

    // Movable (agent-pin/unpinned) priority key: agent pins (tier 1) before
    // unpinned (tier 2), then backlog-priority rank, then document order.
    let movable_key = |idx: usize| -> (u8, u8, usize) {
        let e = &prompts[idx];
        let tier = entry_priority_tier(e);
        let r = if tier < 2 {
            0
        } else {
            entry_do_id(e)
                .and_then(|id| rank.get(&id).copied())
                .unwrap_or(u8::MAX)
        };
        (tier, r, idx)
    };
    let is_operator_pin = |idx: usize| entry_priority_tier(&prompts[idx]) == 0;

    // Plain priority-weighted topological order over ALL prompts (Kahn). Used for
    // the no-operator-pin path and as the blocker-outranks-pin fallback. A
    // dependency cycle emits the remaining prompts in priority order (never
    // dropped).
    let plain_order = || -> Vec<usize> {
        let mut done = vec![false; n];
        let mut order: Vec<usize> = Vec::with_capacity(n);
        for _ in 0..n {
            let mut best: Option<usize> = None;
            for idx in 0..n {
                if done[idx] || prereq[idx].iter().any(|&j| !done[j]) {
                    continue;
                }
                if best.is_none_or(|b| movable_key(idx) < movable_key(b)) {
                    best = Some(idx);
                }
            }
            match best {
                Some(pick) => {
                    done[pick] = true;
                    order.push(pick);
                }
                None => {
                    let mut leftover: Vec<usize> = (0..n).filter(|&i| !done[i]).collect();
                    leftover.sort_by_key(|&i| movable_key(i));
                    order.extend(leftover);
                    break;
                }
            }
        }
        order
    };

    // `#queue-operator-pin-position-lock-dag`: operator pins are position-locked
    // in the DAG order exactly as in `sort_prompts_by_priority` — they keep their
    // document slot while only the movable (agent-pin/unpinned) prompts reorder
    // around them. The earlier DAG tie-break treated an operator pin as tier 0 in
    // the ready set, floating it ahead of earlier prompts whenever `after=#id`
    // edges existed. Anchor operator pins to their slots and fill the remaining
    // slots with the movable prompts in dependency-respecting priority order. If
    // anchoring would violate a dependency edge, the blocker outranks the pin and
    // we fall back to the plain dependency topo so a dependency is never broken.
    let order: Vec<usize> = if !(0..n).any(is_operator_pin) {
        plain_order()
    } else {
        let movable: Vec<usize> = (0..n).filter(|&i| !is_operator_pin(i)).collect();
        let movable_set: std::collections::HashSet<usize> = movable.iter().copied().collect();
        // Topological order over the movable prompts only; operator-pin
        // prerequisites are placed separately at their anchor slots, so treat
        // them as already satisfied here.
        let mut done_m: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let mut movable_seq: Vec<usize> = Vec::with_capacity(movable.len());
        for _ in 0..movable.len() {
            let mut best: Option<usize> = None;
            for &idx in &movable {
                if done_m.contains(&idx) {
                    continue;
                }
                if prereq[idx]
                    .iter()
                    .any(|&j| movable_set.contains(&j) && !done_m.contains(&j))
                {
                    continue;
                }
                if best.is_none_or(|b| movable_key(idx) < movable_key(b)) {
                    best = Some(idx);
                }
            }
            match best {
                Some(pick) => {
                    done_m.insert(pick);
                    movable_seq.push(pick);
                }
                None => {
                    let mut leftover: Vec<usize> = movable
                        .iter()
                        .copied()
                        .filter(|i| !done_m.contains(i))
                        .collect();
                    leftover.sort_by_key(|&i| movable_key(i));
                    movable_seq.extend(leftover);
                    break;
                }
            }
        }
        // Fill: operator pins keep their document slot; movable prompts fill the
        // rest in `movable_seq` order.
        let mut anchored: Vec<usize> = Vec::with_capacity(n);
        let mut mv = movable_seq.into_iter();
        for slot in 0..n {
            if is_operator_pin(slot) {
                anchored.push(slot);
            } else {
                anchored.push(
                    mv.next()
                        .expect("movable prompts fill every non-operator-pin slot"),
                );
            }
        }
        // Validate every dependency edge against the anchored order; if any is
        // violated, the blocker outranks the pin → use the plain dependency topo.
        let mut pos = vec![0usize; n];
        for (p, &i) in anchored.iter().enumerate() {
            pos[i] = p;
        }
        let deps_ok = (0..n).all(|i| prereq[i].iter().all(|&j| pos[j] < pos[i]));
        if deps_ok { anchored } else { plain_order() }
    };

    if order.iter().enumerate().all(|(slot, &i)| slot == i) {
        return None;
    }
    let mut out = entries.to_vec();
    for (slot, &pos) in positions.iter().enumerate() {
        out[pos] = prompts[order[slot]].clone();
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
    rewrite_queue_tag_attrs(tag, |token| !matches!(token, "start" | "go" | "stop"))
}

/// Normalize malformed boolean marker attributes on an `agent:queue` opening tag.
///
/// Bare queue flags (`priority`, `go`, `start`, `stop`, `auto`) must render as
/// tokens, not as `key=true`. Some editor/HTML serializers represent boolean
/// attrs that way, and one malformed form has also appeared as
/// `preset="#name"=true`. Normalize those spellings while preserving token order
/// and quoted preset values.
pub fn normalize_queue_tag_attrs(tag: &str) -> String {
    rewrite_queue_tag_attrs(tag, |_| true)
}

fn rewrite_queue_tag_attrs(tag: &str, keep: impl Fn(&str) -> bool) -> String {
    let Some((tokens, tail)) = queue_tag_tokens(tag) else {
        return tag.to_string();
    };
    let tokens = tokens
        .into_iter()
        .filter_map(|token| {
            let normalized = normalize_queue_tag_token(&token);
            keep(queue_tag_token_key(&normalized)).then_some(normalized)
        })
        .collect::<Vec<_>>();
    format!("<!-- {} -->{}", tokens.join(" "), tail)
}

fn queue_tag_tokens(tag: &str) -> Option<(Vec<String>, &str)> {
    let close_idx = tag.find("-->")?;
    let core = &tag[..close_idx + 3];
    let tail = &tag[close_idx + 3..];
    let inner = core
        .trim_start_matches("<!--")
        .trim_end_matches("-->")
        .trim();
    Some((split_marker_tokens(inner), tail))
}

fn split_marker_tokens(inner: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for ch in inner.chars() {
        if ch.is_whitespace() && quote.is_none() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            continue;
        }
        if matches!(ch, '"' | '\'') {
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            }
        }
        current.push(ch);
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn normalize_queue_tag_token(token: &str) -> String {
    let Some((key, value)) = token.split_once('=') else {
        return token.to_string();
    };
    if is_queue_boolean_attr(key) && value.eq_ignore_ascii_case("true") {
        return key.to_string();
    }
    if key == "preset"
        && let Some(stripped) = strip_malformed_true_suffix(value)
    {
        return format!("{key}={stripped}");
    }
    token.to_string()
}

fn queue_tag_token_key(token: &str) -> &str {
    token.split_once('=').map(|(key, _)| key).unwrap_or(token)
}

fn is_queue_boolean_attr(key: &str) -> bool {
    matches!(key, "auto" | "priority" | "go" | "start" | "stop")
}

fn strip_malformed_true_suffix(value: &str) -> Option<&str> {
    let stripped = value.strip_suffix("=true")?;
    if (stripped.starts_with('"') && stripped.ends_with('"'))
        || (stripped.starts_with('\'') && stripped.ends_with('\''))
    {
        Some(stripped)
    } else {
        None
    }
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

/// Collapse duplicate live `Prompt` entries that target the same queue identity,
/// keeping the first occurrence. We only collapse duplicates for explicit `do
/// [#id]`/`do #id` identity prompts because that pattern is expected from CRDT
/// merge replay and backlog sync, while still allowing user-authored duplicate
/// free-text prompts to remain.
///
/// `Completed`/`Preset`/fence entries are left untouched.
pub fn dedup_live_prompts(entries: &[QueueEntry]) -> Option<Vec<QueueEntry>> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut deduped = Vec::with_capacity(entries.len());
    let mut dropped = false;
    for entry in entries {
        if let QueueEntry::Prompt(prompt) = entry
            && let Some(key) = dedup_key_for_prompt(prompt)
            && !seen.insert(key)
        {
            dropped = true;
            continue;
        }
        deduped.push(entry.clone());
    }
    if dropped { Some(deduped) } else { None }
}

fn dedup_key_for_prompt(prompt: &QueuePrompt) -> Option<String> {
    let trimmed = prompt.text.trim().to_ascii_lowercase();
    if !(trimmed.starts_with("do [#") || trimmed.starts_with("do #")) {
        return None;
    }
    do_prompt_id(&trimmed)
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
mod tests;
