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
    Ok(parse_spans(body)?
        .into_iter()
        .map(|(entry, _)| entry)
        .collect())
}

/// Parse a queue body into entries paired with their byte range within `body`.
///
/// Each range spans the entry's full source extent — including fence open/close
/// lines and the trailing newline — so a caller can excise an exact entry by
/// range. [`parse`] is the entry-only thin wrapper over this. This is the SINGLE
/// source of queue-head segmentation (multiline `---`/```/~~~ fenced Prompt heads
/// included): any second enumerator that disagrees lets a head class evade the
/// strike/consume path. That divergence is exactly how multiline `:round_pushpin:`
/// paste blocks (surfaced here as `Prompt { multiline: true }`, but invisible to
/// the bullet-only `markdown_ast` `item_nodes`) accumulated in the queue forever
/// with no way for `queue prune-noise` to clear them (#qnoise-multiline-strike).
pub fn parse_spans(body: &str) -> Result<Vec<(QueueEntry, std::ops::Range<usize>)>> {
    let body_len = body.len();
    // Mirror `str::lines()` line content while retaining each line's byte start,
    // so entry ranges are exact even across `\r\n` and a missing trailing newline.
    let mut lines: Vec<&str> = Vec::new();
    let mut line_starts: Vec<usize> = Vec::new();
    let mut pos = 0usize;
    for raw in body.split_inclusive('\n') {
        line_starts.push(pos);
        let without_nl = raw.strip_suffix('\n').unwrap_or(raw);
        lines.push(without_nl.strip_suffix('\r').unwrap_or(without_nl));
        pos += raw.len();
    }
    let span = |start_line: usize, end_line: usize| -> std::ops::Range<usize> {
        let start = line_starts.get(start_line).copied().unwrap_or(body_len);
        let end = line_starts.get(end_line).copied().unwrap_or(body_len);
        start..end
    };

    let mut entries: Vec<(QueueEntry, std::ops::Range<usize>)> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let start_i = i;
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        if crate::queue_command::is_slash_command(trimmed) {
            entries.push((
                QueueEntry::Prompt(QueuePrompt {
                    text: trimmed.to_string(),
                    multiline: false,
                }),
                span(start_i, start_i + 1),
            ));
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
            let entry = if let Some(completed) = parse_completed_inline(rest) {
                QueueEntry::Completed(QueuePrompt {
                    text: completed.to_string(),
                    multiline: false,
                })
            } else if is_reference_directive(rest) {
                // Optional-`do` grammar (Stage 1): a `re [#id]` / `re #id` line
                // *references* a tracked id without executing it. Preserve it
                // verbatim as `Freeform` so it is never run, synced, or reaped.
                QueueEntry::Freeform(line.to_string())
            } else {
                QueueEntry::Prompt(QueuePrompt {
                    text: rest.to_string(),
                    multiline: false,
                })
            };
            entries.push((entry, span(start_i, start_i + 1)));
            i += 1;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("preset ") {
            let preset = rest.trim();
            if !preset.is_empty() {
                entries.push((QueueEntry::Preset(preset.to_string()), span(start_i, start_i + 1)));
                i += 1;
                continue;
            }
        }

        if let Some(rest) = trimmed.strip_prefix("dispatch ") {
            let preset = rest.trim();
            if !preset.is_empty() {
                entries.push((
                    QueueEntry::Dispatch(preset.to_string()),
                    span(start_i, start_i + 1),
                ));
                i += 1;
                continue;
            }
        }

        if is_start_fence(trimmed) {
            let datetime = parse_start_datetime(trimmed);
            entries.push((QueueEntry::StartFence(datetime), span(start_i, start_i + 1)));
            i += 1;
            continue;
        }

        if is_stop_fence(trimmed) {
            entries.push((QueueEntry::StopFence, span(start_i, start_i + 1)));
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
                    let entry = if completed {
                        QueueEntry::Completed(prompt)
                    } else {
                        QueueEntry::Prompt(prompt)
                    };
                    entries.push((entry, span(start_i, close_idx + 1)));
                }
                i = close_idx + 1;
                continue;
            }
            entries.push((QueueEntry::Freeform(line.to_string()), span(start_i, start_i + 1)));
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
                    entries.push((
                        QueueEntry::Prompt(QueuePrompt {
                            text,
                            multiline: true,
                        }),
                        span(start_i, close_idx + 1),
                    ));
                }
                i = close_idx + 1;
                continue;
            }
            entries.push((QueueEntry::Freeform(line.to_string()), span(start_i, start_i + 1)));
            i += 1;
            continue;
        }

        // Unrecognized line: preserve verbatim instead of failing the parse so
        // queue consume/resume/dispatch guards stay resilient to a polluted
        // queue body (#jb-run-agent-doc-response-queue-contamination). The line
        // is preserved as-is and never treated as an actionable item.
        entries.push((QueueEntry::Freeform(line.to_string()), span(start_i, start_i + 1)));
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

/// True when a `Freeform` queue entry is pasted **noise** that `queue prune-noise`
/// should excise (#qnoise-multiline-strike). The parser routes every unrecognized
/// line to `Freeform` so a polluted queue cannot break parsing of the real items
/// below it — but that same bucket also accumulates operator-pasted console/agent
/// evidence (a ``` code fence and the prose around it) that never drains and can
/// never be struck. Everything reaching `Freeform` is pasted noise EXCEPT two
/// legitimate shapes that must be preserved: a structural separator / control fence
/// (blank, `---`, `~~~…` — an unpaired bare `---`/`~~~` the parser keeps verbatim),
/// or an optional-`do` `re [#id]` / `re #id` reference (possibly bulleted). A ```
/// code-fence line, a `:pushpin:`/prose head, or a raw console line is noise.
pub(crate) fn is_noise_freeform_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed == "---" || trimmed.starts_with("~~~") {
        return false;
    }
    // A `--- start`/`--- stop`/`--- start at …` control line is recognized as a
    // fence entry, never `Freeform`, but guard the prefix anyway for resilience.
    if trimmed.starts_with("--- ") {
        return false;
    }
    let de_bulleted = trimmed.strip_prefix("- ").unwrap_or(trimmed);
    if is_reference_directive(de_bulleted) {
        return false;
    }
    true
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

/// Apply the canonical operator pin to a head **idempotently** (`#pushpinaccum`).
///
/// A queue head carries exactly **one** leading priority marker. This strips any
/// existing leading operator/agent pin marker(s) (`:pushpin:` / `:round_pushpin:`
/// in any spelling) plus surrounding whitespace, then prefixes a single
/// `:pushpin:`. Two accumulation bugs are closed:
/// - **Promotion dedup:** promoting a `:round_pushpin:` (agent) head to a
///   `:pushpin:` (operator) head REPLACES the marker instead of stacking it, so
///   the result is `:pushpin: do [#x]`, never `:pushpin: :round_pushpin: do [#x]`.
/// - **Re-pin no-op:** re-pinning an already-`:pushpin:` head yields the same
///   text (idempotent), never `:pushpin: :pushpin: do [#x]`.
fn apply_operator_pin(text: &str) -> String {
    format!("{} {}", PRIORITIZED_MARKER, strip_priority_markers(text))
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
                // Idempotent promotion: drop any existing agent pin so an operator
                // reorder of a `:round_pushpin:` head yields a single `:pushpin:`,
                // not `:pushpin: :round_pushpin:` (`#pushpinaccum`).
                prompt.text = apply_operator_pin(&prompt.text);
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
        prompt.text = apply_operator_pin(&prompt.text);
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

/// True when `entry` is a `do [#id]` prompt whose id is in `backlog_sourced`
/// (the active-backlog ids the binary syncs into the queue this cycle via the
/// `queue` attribute). Such prompts are **append-stable**
/// (`#backlog-queue-append-stable`): the `priority` sort holds them in a tier
/// AFTER the pre-existing unpinned movable prompts (manual operator lines and
/// free-text prompts that were already in the queue), instead of interleaving
/// them by backlog rank. Operator/agent pins are unaffected — a pinned backlog
/// item is not held to the tail. The set is keyed off the *active backlog id*
/// (not a per-cycle "new this cycle" diff) so a previously-synced item stays
/// append-stable on later cycles instead of floating up once it is no longer
/// "fresh".
fn entry_is_backlog_sourced(
    entry: &QueueEntry,
    backlog_sourced: &std::collections::HashSet<String>,
) -> bool {
    if backlog_sourced.is_empty() {
        return false;
    }
    // Pinned prompts are never held to the tail — an operator/agent pin is an
    // explicit position signal that outranks append-stability.
    if entry_priority_tier(entry) != 2 {
        return false;
    }
    entry_do_id(entry).is_some_and(|id| backlog_sourced.contains(&id))
}

pub fn sort_prompts_by_priority(
    entries: &[QueueEntry],
    rank: &std::collections::HashMap<String, u8>,
    backlog_sourced: &std::collections::HashSet<String>,
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
    //
    // `#backlog-queue-append-stable`: backlog-sourced unpinned prompts (ids in
    // `backlog_sourced`) get an extra group key (`1`) so they sort AFTER the
    // pre-existing unpinned prompts (group `0`) instead of interleaving by
    // backlog rank. Within the backlog-sourced group, backlog-priority rank then
    // document order are preserved. The default `queue` append therefore stays
    // appended even under `priority` — a freshly-scheduled item never floats
    // above a non-annotated free-text prompt that was already in the queue.
    let key = |idx: usize| -> (u8, u8, u8) {
        let e = &prompts[idx];
        let tier = entry_priority_tier(e); // 1 (agent pin) or 2 (unpinned) here
        if tier < 2 {
            (tier, 0, 0)
        } else {
            let group = u8::from(entry_is_backlog_sourced(e, backlog_sourced));
            let r = entry_do_id(e)
                .and_then(|id| rank.get(&id).copied())
                .unwrap_or(u8::MAX);
            (2, group, r)
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
/// agent pin, then append-stable group (pre-existing before backlog-sourced),
/// then backlog-priority rank, then document order. Dependencies
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
    backlog_sourced: &std::collections::HashSet<String>,
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
    // unpinned (tier 2), then — for unpinned prompts — the append-stable group
    // (`#backlog-queue-append-stable`: pre-existing `0` before backlog-sourced
    // `1`), then backlog-priority rank, then document order. The dependency
    // topological pass still always wins, so a blocker that is backlog-sourced
    // can still precede a pre-existing dependent; append-stability only governs
    // the tie-break among edge-free / ready prompts.
    let movable_key = |idx: usize| -> (u8, u8, u8, usize) {
        let e = &prompts[idx];
        let tier = entry_priority_tier(e);
        if tier < 2 {
            (tier, 0, 0, idx)
        } else {
            let group = u8::from(entry_is_backlog_sourced(e, backlog_sourced));
            let r = entry_do_id(e)
                .and_then(|id| rank.get(&id).copied())
                .unwrap_or(u8::MAX);
            (tier, group, r, idx)
        }
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
    // Strip a leading operator/agent pin marker (`:pushpin:` / `:round_pushpin:`,
    // any spelling) before testing the `do [#id]` identity prefix
    // (`#qdup-pin-prefix`). The pin is a priority annotation, not part of the
    // queue identity, so a pinned head (`:pushpin: do [#id]`) must dedupe against
    // its unpinned twin (`do [#id]`) — otherwise a re-pinned id-backed head
    // accumulates as a visible duplicate instead of being collapsed. A plain
    // capitalized `Do [#id]` already normalizes via `to_ascii_lowercase`; the pin
    // prefix was the sole gap.
    let trimmed = strip_priority_markers(&prompt.text).to_ascii_lowercase();
    if !(trimmed.starts_with("do [#") || trimmed.starts_with("do #")) {
        return None;
    }
    do_prompt_id(&trimmed)
}

/// Collapse duplicate **bare id-reference** queue heads (`[#id]` / `#id` with
/// nothing trailing — the pure reference form the backlog→queue mirror and CRDT
/// replay emit), keeping the first occurrence (#qdup-bare-id).
///
/// Unlike [`dedup_live_prompts`], this deliberately does NOT touch `do [#id]`
/// **directive** heads: a duplicate `do [#id]` can be intentional "run it twice"
/// user queue intent (`#queue-dedup-destroys-intentional-duplicates`), but a
/// duplicate bare reference head is always a mirror/replay artifact — the
/// operator-reported "agent-doc duplicated my queue items" bug (`[#sqedit-race]` /
/// `[#qpausemix-verify]` appearing twice). Free-text prompts — including a directive
/// that merely cites an id with trailing text (`#id continue the drain`) — are
/// preserved, as are multiline blocks. `Completed`/`Preset`/fence entries untouched.
pub fn dedup_bare_id_reference_heads(entries: &[QueueEntry]) -> Option<Vec<QueueEntry>> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut deduped = Vec::with_capacity(entries.len());
    let mut dropped = false;
    for entry in entries {
        if let QueueEntry::Prompt(prompt) = entry
            && let Some(key) = bare_id_reference_key(prompt)
            && !seen.insert(key)
        {
            dropped = true;
            continue;
        }
        deduped.push(entry.clone());
    }
    if dropped { Some(deduped) } else { None }
}

fn bare_id_reference_key(prompt: &QueuePrompt) -> Option<String> {
    if prompt.multiline {
        return None;
    }
    let trimmed = strip_priority_markers(&prompt.text).trim().to_ascii_lowercase();
    // ONLY a pure `[#id]` / `#id` head — nothing trailing. `do [#id]` is excluded
    // (it starts with `do`, so neither prefix matches) and a free-text directive
    // citing an id has trailing text that fails the `id == token` whole-match check.
    let token = trimmed
        .strip_prefix("[#")
        .and_then(|rest| rest.strip_suffix(']'))
        .or_else(|| trimmed.strip_prefix('#'))?
        .trim();
    let id: String = token
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if !id.is_empty() && id == token {
        Some(id)
    } else {
        None
    }
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
    fn strip_priority_markers_normalizes_pin_for_identity() {
        // #queue-consume-pushpin-normalization: a head differing only by a
        // cosmetic pin annotation must normalize to the same identity, so the
        // queue-consume head-equality check does not spuriously fail when the
        // snapshot holds the unpinned spelling and the document the pinned one.
        let bare = "do [#md-ast-document-model]. I like the tsift AST.";
        assert_eq!(strip_priority_markers(bare), bare);
        assert_eq!(strip_priority_markers(":pushpin: do [#x]"), "do [#x]");
        assert_eq!(strip_priority_markers("  :pushpin:   do [#x]"), "do [#x]");
        assert_eq!(strip_priority_markers(":round_pushpin: do [#x]"), "do [#x]");
        assert_eq!(strip_priority_markers("**prioritized** do [#x]"), "do [#x]");
        assert_eq!(strip_priority_markers("📌 do [#x]"), "do [#x]");
        // The exact session repro: pinned vs unpinned spellings are equal.
        assert_eq!(
            strip_priority_markers(":pushpin: do [#md-ast-document-model]"),
            strip_priority_markers("do [#md-ast-document-model]")
        );
        // A non-pin free-text head is untouched (no false stripping).
        assert_eq!(
            strip_priority_markers("Should tsift be a hard dependency?"),
            "Should tsift be a hard dependency?"
        );
    }

    #[test]
    fn annotate_agent_priority_promotions_marks_promoted_prompt() {
        let before = parse("- do [#low]\n- do [#high]\n").unwrap();
        let after = parse("- do [#high]\n- do [#low]\n").unwrap();
        let marked =
            annotate_agent_priority_promotions(&before, &after).expect("promotion should annotate");
        assert_eq!(
            render(&marked),
            "- :round_pushpin: do [#high]\n- do [#low]\n"
        );
    }

    #[test]
    fn annotate_operator_priority_reorders_marks_manually_moved_prompt() {
        let snapshot = parse("- do [#a]\n- do [#b]\n- do [#c]\n").unwrap();
        let current = parse("- do [#c]\n- do [#a]\n- do [#b]\n").unwrap();

        let marked = annotate_operator_priority_reorders(&snapshot, &current)
            .expect("manual promotion should annotate");

        assert_eq!(
            render(&marked),
            "- :pushpin: do [#c]\n- do [#a]\n- do [#b]\n"
        );
    }

    #[test]
    fn annotate_operator_priority_reorders_upgrades_agent_pin() {
        // #pushpinaccum: promoting a `:round_pushpin:` (agent) head to an operator
        // pin REPLACES the marker — the result carries a single `:pushpin:`, never
        // the accumulated `:pushpin: :round_pushpin:`.
        let snapshot = parse("- do [#a]\n- :round_pushpin: do [#b]\n").unwrap();
        let current = parse("- :round_pushpin: do [#b]\n- do [#a]\n").unwrap();

        let marked = annotate_operator_priority_reorders(&snapshot, &current)
            .expect("operator move should add operator pin");

        assert_eq!(render(&marked), "- :pushpin: do [#b]\n- do [#a]\n");
    }

    #[test]
    fn annotate_operator_priority_reorders_repin_is_idempotent() {
        // #pushpinaccum: an already-`:pushpin:` head that the operator moves earlier
        // is left as-is (the existing-operator-pin guard) — it never stacks a second
        // `:pushpin:`. Move an unpinned neighbor so the function returns Some.
        let snapshot = parse("- :pushpin: do [#b]\n- do [#a]\n- do [#c]\n").unwrap();
        let current = parse("- :pushpin: do [#b]\n- do [#c]\n- do [#a]\n").unwrap();

        let marked = annotate_operator_priority_reorders(&snapshot, &current)
            .expect("the moved unpinned neighbor pins");

        assert_eq!(
            render(&marked),
            "- :pushpin: do [#b]\n- :pushpin: do [#c]\n- do [#a]\n"
        );
    }

    #[test]
    fn apply_operator_pin_is_idempotent_and_dedupes() {
        // #pushpinaccum: one leading marker, always.
        assert_eq!(apply_operator_pin("do [#x]"), ":pushpin: do [#x]");
        assert_eq!(apply_operator_pin(":pushpin: do [#x]"), ":pushpin: do [#x]");
        assert_eq!(
            apply_operator_pin(":round_pushpin: do [#x]"),
            ":pushpin: do [#x]"
        );
        assert_eq!(
            apply_operator_pin(":pushpin: :round_pushpin: do [#x]"),
            ":pushpin: do [#x]"
        );
    }

    #[test]
    fn annotate_operator_priority_reorders_ignores_new_and_later_prompts() {
        let snapshot = parse("- do [#a]\n- do [#b]\n").unwrap();
        let current = parse("- do [#new]\n- do [#b]\n- do [#a]\n").unwrap();

        assert!(annotate_operator_priority_reorders(&snapshot, &current).is_none());
    }

    #[test]
    fn annotate_manual_queue_additions_pins_operator_added_unpinned_line() {
        // #7r2s: the operator typed a brand-new `do [#manual]` line with no pin.
        // It is absent from the snapshot and was NOT appended by the backlog sync,
        // so it is auto-pinned with operator priority and stays at its slot.
        let snapshot = parse("- do [#a]\n").unwrap();
        let current = parse("- do [#manual]\n- do [#a]\n").unwrap();
        let synced: std::collections::HashSet<String> = std::collections::HashSet::new();

        let marked = annotate_manual_queue_additions(&snapshot, &current, &synced)
            .expect("a new operator-added line must be auto-pinned");
        assert_eq!(render(&marked), "- :pushpin: do [#manual]\n- do [#a]\n");
    }

    #[test]
    fn annotate_manual_queue_additions_skips_backlog_synced_and_pinned() {
        // A new line the binary appended from the backlog this cycle (#synced) is
        // NOT auto-pinned; an already-pinned new line is left as-is; an existing
        // (snapshot) line is untouched.
        let snapshot = parse("- do [#a]\n").unwrap();
        let current = parse("- do [#synced]\n- :round_pushpin: do [#pinned]\n- do [#a]\n").unwrap();
        let synced: std::collections::HashSet<String> =
            ["synced".to_string()].into_iter().collect();

        assert!(
            annotate_manual_queue_additions(&snapshot, &current, &synced).is_none(),
            "binary-synced and already-pinned new lines must not be auto-pinned"
        );
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
        let sorted = sort_prompts_by_priority(&entries, &rank, &std::collections::HashSet::new())
            .expect("order should change");
        assert_eq!(render(&sorted), "- do [#b]\n- do [#c]\n- do [#a]\n");
    }

    #[test]
    fn sort_prompts_by_priority_keeps_non_prompts_in_place() {
        let entries = parse("preset spec\n- do [#a]\n- do [#b]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 2u8);
        rank.insert("b".to_string(), 1u8);
        let sorted = sort_prompts_by_priority(&entries, &rank, &std::collections::HashSet::new())
            .expect("order should change");
        // preset stays at index 0; prompts reorder among themselves.
        assert_eq!(render(&sorted), "preset spec\n- do [#b]\n- do [#a]\n");
    }

    #[test]
    fn sort_prompts_by_priority_idempotent_when_ordered() {
        let entries = parse("- do [#a]\n- do [#b]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8);
        rank.insert("b".to_string(), 2u8);
        assert!(
            sort_prompts_by_priority(&entries, &rank, &std::collections::HashSet::new()).is_none()
        );
    }

    #[test]
    fn backlog_sourced_item_appends_after_pre_existing_freetext() {
        // #backlog-queue-append-stable: a backlog-sourced `do [#a]` (id in the
        // active backlog) stays APPENDED after a pre-existing non-annotated
        // free-text prompt under `priority`, instead of floating above it by rank.
        // The operator directive: "append, not prepend, even with non-annotated
        // items in the queue."
        let entries = parse("- review the spec draft\n- do [#a]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8); // best rank — would float to top if not held
        let backlog: std::collections::HashSet<String> = ["a".to_string()].into_iter().collect();
        // Held to the tail → already in document order → no reorder.
        assert!(sort_prompts_by_priority(&entries, &rank, &backlog).is_none());
        // Contrast: without append-stability (empty set) the backlog item DOES
        // float above the free-text prompt — the pre-fix prepend behavior.
        let floated = sort_prompts_by_priority(&entries, &rank, &std::collections::HashSet::new())
            .expect("empty set → backlog item floats up by rank");
        assert_eq!(render(&floated), "- do [#a]\n- review the spec draft\n");
    }

    #[test]
    fn backlog_sourced_group_preserves_rank_after_pre_existing() {
        // The backlog-sourced group sorts after the pre-existing free-text prompt,
        // but keeps backlog-rank order among its own members.
        let entries = parse("- manual note\n- do [#a]\n- do [#b]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 5u8);
        rank.insert("b".to_string(), 1u8); // better rank → before #a within the group
        let backlog: std::collections::HashSet<String> =
            ["a".to_string(), "b".to_string()].into_iter().collect();
        let sorted = sort_prompts_by_priority(&entries, &rank, &backlog).expect("group reorders");
        assert_eq!(render(&sorted), "- manual note\n- do [#b]\n- do [#a]\n");
    }

    #[test]
    fn pinned_backlog_sourced_item_is_exempt_from_append_stable() {
        // A pin is an explicit position signal that outranks append-stability: an
        // operator-pinned backlog item keeps its slot and is not dragged to the
        // tail (#7r2s pins genuinely operator-typed lines, so they are exempt).
        let entries = parse("- __prioritized__ do [#a]\n- free text prompt\n").unwrap();
        let rank = std::collections::HashMap::new();
        let backlog: std::collections::HashSet<String> = ["a".to_string()].into_iter().collect();
        // Operator pin holds slot 0; the lone free-text prompt has no peer to swap
        // with → nothing moves (the pinned backlog item is NOT pushed past it).
        assert!(sort_prompts_by_priority(&entries, &rank, &backlog).is_none());
    }

    #[test]
    fn backlog_sourced_append_stable_applies_in_dag_path() {
        // The append-stable group key also governs the auto-dag tie-break: with a
        // real `after=` edge, a backlog-sourced dependent still sorts after the
        // pre-existing free-text prompt among the ready set.
        let entries = parse("- free text\n- do [#a]\n- do [#b]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 5u8);
        rank.insert("b".to_string(), 1u8);
        let mut deps: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        deps.insert("a".to_string(), vec!["b".to_string()]); // #a after #b
        let backlog: std::collections::HashSet<String> =
            ["a".to_string(), "b".to_string()].into_iter().collect();
        let sorted =
            sort_prompts_by_dag(&entries, &rank, &deps, &backlog).expect("dep edge reorders");
        // #b precedes #a (edge), and the whole backlog group stays after the
        // pre-existing free-text prompt.
        assert_eq!(render(&sorted), "- free text\n- do [#b]\n- do [#a]\n");
    }

    #[test]
    fn operator_pin_stays_in_place_while_unpinned_reorder() {
        // #queue-operator-pin-position-lock: a `__prioritized__` operator pin
        // stays at its authored slot; only the unpinned prompts reorder around it.
        let entries = parse("- do [#c]\n- __prioritized__ do [#b]\n- do [#a]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8); // best rank → first among unpinned
        rank.insert("b".to_string(), 9u8); // worst rank, but pinned → frozen in middle
        rank.insert("c".to_string(), 2u8);
        let sorted = sort_prompts_by_priority(&entries, &rank, &std::collections::HashSet::new())
            .expect("unpinned reorder");
        // Pin #b holds slot 1; unpinned #a (rank1) and #c (rank2) fill slots 0,2.
        assert_eq!(
            render(&sorted),
            "- do [#a]\n- __prioritized__ do [#b]\n- do [#c]\n"
        );
    }

    #[test]
    fn operator_pin_at_bottom_not_floated_to_top() {
        // The exact operator complaint: a pinned item at the bottom must NOT be
        // hoisted to the top by the priority attribute — it stays put.
        let entries = parse("- do [#a]\n- __prioritized__ do [#b]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8);
        rank.insert("b".to_string(), 9u8);
        // #a is already in rank order at slot 0 and the pin is anchored at slot 1
        // → nothing moves.
        assert!(
            sort_prompts_by_priority(&entries, &rank, &std::collections::HashSet::new()).is_none()
        );
    }

    #[test]
    fn operator_pins_hold_their_slots_unpinned_reorder_around() {
        // Multiple operator pins each hold their own slot; the unpinned prompts
        // reorder among the remaining slots in rank order.
        let entries =
            parse("- do [#a]\n- __prioritized__ [#z]\n- do [#b]\n- __prioritized__ [#y]\n")
                .unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 2u8);
        rank.insert("b".to_string(), 1u8);
        let sorted = sort_prompts_by_priority(&entries, &rank, &std::collections::HashSet::new())
            .expect("unpinned reorder");
        // Pins #z, #y stay at slots 1,3; unpinned #b (rank1), #a (rank2) fill 0,2.
        assert_eq!(
            render(&sorted),
            "- do [#b]\n- __prioritized__ [#z]\n- do [#a]\n- __prioritized__ [#y]\n"
        );
    }

    #[test]
    fn prioritized_marker_release_reverts_to_rank_order() {
        // Deleting the marker drops the item back into rank-ordered position.
        let entries = parse("- do [#a]\n- do [#b]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 2u8);
        rank.insert("b".to_string(), 1u8);
        let sorted = sort_prompts_by_priority(&entries, &rank, &std::collections::HashSet::new())
            .expect("rank reorders");
        assert_eq!(render(&sorted), "- do [#b]\n- do [#a]\n");
    }

    #[test]
    fn operator_pin_position_locked_with_empty_rank_map() {
        // No backlog priority at all: the operator pin stays exactly where placed
        // (previously it floated to the top — that is the behavior being removed).
        let entries = parse("- do [#a]\n- __prioritized__ do [#b]\n").unwrap();
        let rank = std::collections::HashMap::new();
        assert!(
            sort_prompts_by_priority(&entries, &rank, &std::collections::HashSet::new()).is_none()
        );
    }

    #[test]
    fn is_prioritized_detects_marker() {
        assert!(is_prioritized("__prioritized__ do [#x]"));
        assert!(is_prioritized("  __prioritized__ [#x]"));
        assert!(!is_prioritized("do [#x]"));
        assert!(!is_prioritized("not __prioritized__ here"));
    }

    #[test]
    fn operator_pin_anchored_while_agent_pin_floats_among_movable() {
        // #queue-agent-vs-operator-pin-tier + #queue-operator-pin-position-lock:
        // the operator pin (__) is anchored at its slot; among the remaining
        // (movable) slots the agent pin (_) still floats above unpinned.
        let entries = parse(concat!(
            "- do [#a]\n",
            "- _prioritized_ do [#b]\n",
            "- __prioritized__ do [#c]\n",
        ))
        .unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8); // best rank, but unpinned
        rank.insert("b".to_string(), 5u8);
        rank.insert("c".to_string(), 9u8); // operator-pinned → stays at slot 2
        let sorted = sort_prompts_by_priority(&entries, &rank, &std::collections::HashSet::new())
            .expect("agent pin floats");
        // Operator pin #c stays at the bottom (slot 2); agent pin #b floats above
        // unpinned #a in the two movable slots.
        assert_eq!(
            render(&sorted),
            "- _prioritized_ do [#b]\n- do [#a]\n- __prioritized__ do [#c]\n"
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
    fn pin_markers_accept_both_asterisk_and_underscore_emphasis() {
        // Operator = strong emphasis (** / __); agent = italic emphasis (* / _).
        assert!(is_prioritized("**prioritized** do [#x]"));
        assert!(is_prioritized("__prioritized__ do [#x]"));
        assert!(is_agent_prioritized("*prioritized* do [#x]"));
        assert!(is_agent_prioritized("_prioritized_ do [#x]"));
        // Strong emphasis is operator, never agent — for both spellings.
        assert!(!is_agent_prioritized("**prioritized** do [#x]"));
        assert!(!is_agent_prioritized("__prioritized__ do [#x]"));
    }

    #[test]
    fn pin_shortcode_aliases_resolve_to_tiers() {
        // Operator aliases: :pin:, :pushpin:, 📌, **pin**, __pin__.
        for m in [
            "**prioritized**",
            "__prioritized__",
            "**pin**",
            "__pin__",
            ":pin:",
            ":pushpin:",
            "📌",
        ] {
            assert!(is_prioritized(&format!("{m} do [#x]")), "operator: {m}");
            assert!(
                !is_agent_prioritized(&format!("{m} do [#x]")),
                "not agent: {m}"
            );
        }
        // Agent aliases: _pin_, :round_pushpin:, 📍, *pin*, *prioritized*.
        for m in [
            "*prioritized*",
            "_prioritized_",
            "*pin*",
            "_pin_",
            ":round_pushpin:",
            "📍",
        ] {
            assert!(is_agent_prioritized(&format!("{m} do [#x]")), "agent: {m}");
            assert!(
                !is_prioritized(&format!("{m} do [#x]")),
                "not operator: {m}"
            );
        }
        // Tier ordering with emoji shortcodes: the operator pin (:pushpin:) is
        // position-locked at its slot; the agent pin (:round_pushpin:) floats
        // above the unpinned prompt among the remaining slots.
        let entries = parse(concat!(
            "- do [#a]\n",
            "- :round_pushpin: do [#b]\n",
            "- :pushpin: do [#c]\n",
        ))
        .unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8);
        let sorted = sort_prompts_by_priority(&entries, &rank, &std::collections::HashSet::new())
            .expect("tiers reorder");
        assert_eq!(
            render(&sorted),
            "- :round_pushpin: do [#b]\n- do [#a]\n- :pushpin: do [#c]\n"
        );
    }

    #[test]
    fn dag_orders_dependency_before_dependent() {
        // #queue-auto-dag-priority: `after=#a` forces #a before #b even though #b
        // has the better priority rank.
        let entries = parse("- do [#b]\n- do [#a]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("b".to_string(), 1u8); // best rank
        rank.insert("a".to_string(), 9u8);
        let mut deps = std::collections::HashMap::new();
        deps.insert("b".to_string(), vec!["a".to_string()]); // b after a
        let sorted = sort_prompts_by_dag(&entries, &rank, &deps, &std::collections::HashSet::new())
            .expect("dep reorders");
        assert_eq!(render(&sorted), "- do [#a]\n- do [#b]\n");
    }

    #[test]
    fn dag_blocker_outranks_pin() {
        // A pinned item that depends on an unpinned item cannot float above it.
        let entries = parse("- :pushpin: do [#b]\n- do [#a]\n").unwrap();
        let rank = std::collections::HashMap::new();
        let mut deps = std::collections::HashMap::new();
        deps.insert("b".to_string(), vec!["a".to_string()]); // pinned b after a
        let sorted = sort_prompts_by_dag(&entries, &rank, &deps, &std::collections::HashSet::new())
            .expect("blocker wins");
        assert_eq!(render(&sorted), "- do [#a]\n- :pushpin: do [#b]\n");
    }

    #[test]
    fn dag_operator_pin_position_locked_with_movable_edges() {
        // #queue-operator-pin-position-lock-dag: with `after=#id` edges among the
        // movable prompts, the operator pin must stay anchored at its document
        // slot — not float to the front by tier — while the movable prompts
        // reorder around it to satisfy the dependency.
        let entries = parse("- do [#y] after=#x\n- :pushpin: do [#p]\n- do [#x]\n").unwrap();
        let rank = std::collections::HashMap::new();
        let deps = std::collections::HashMap::new();
        let sorted = sort_prompts_by_dag(&entries, &rank, &deps, &std::collections::HashSet::new())
            .expect("movable edge reorders around pin");
        assert_eq!(
            render(&sorted),
            "- do [#x]\n- :pushpin: do [#p]\n- do [#y] after=#x\n"
        );
    }

    #[test]
    fn dag_operator_pin_no_spurious_reorder_with_edges() {
        // The pin is already at its anchored slot and the movable order already
        // satisfies the edge → the DAG sort must not spuriously float the pin
        // forward (the pre-fix bug returned `#p, #x, #y`). It returns None.
        let entries = parse("- do [#x]\n- :pushpin: do [#p]\n- do [#y] after=#x\n").unwrap();
        let rank = std::collections::HashMap::new();
        let deps = std::collections::HashMap::new();
        assert!(
            sort_prompts_by_dag(&entries, &rank, &deps, &std::collections::HashSet::new())
                .is_none(),
            "operator pin at its slot with a satisfied edge must not be reordered"
        );
    }

    #[test]
    fn dag_returns_none_without_edges() {
        let entries = parse("- do [#a]\n- do [#b]\n").unwrap();
        let rank = std::collections::HashMap::new();
        let deps = std::collections::HashMap::new();
        assert!(
            sort_prompts_by_dag(&entries, &rank, &deps, &std::collections::HashSet::new())
                .is_none()
        );
    }

    #[test]
    fn dag_inline_after_token_on_queue_prompt() {
        // `after=#a` declared inline on the queue prompt text (not via backlog).
        let entries = parse("- do [#b] after=#a\n- do [#a]\n").unwrap();
        let rank = std::collections::HashMap::new();
        let deps = std::collections::HashMap::new();
        let sorted = sort_prompts_by_dag(&entries, &rank, &deps, &std::collections::HashSet::new())
            .expect("inline dep reorders");
        assert_eq!(render(&sorted), "- do [#a]\n- do [#b] after=#a\n");
    }

    #[test]
    fn dag_cycle_does_not_drop_prompts() {
        // a after b, b after a → cycle; both still emitted (priority order).
        let entries = parse("- do [#a]\n- do [#b]\n").unwrap();
        let rank = std::collections::HashMap::new();
        let mut deps = std::collections::HashMap::new();
        deps.insert("a".to_string(), vec!["b".to_string()]);
        deps.insert("b".to_string(), vec!["a".to_string()]);
        let sorted = sort_prompts_by_dag(&entries, &rank, &deps, &std::collections::HashSet::new());
        // Either reordered or None (already-ordered), but never fewer prompts.
        if let Some(s) = sorted {
            assert_eq!(prompts(&s).len(), 2);
        }
    }

    #[test]
    fn completed_item_renders_double_tilde_strikethrough() {
        // #queue-strike-on-complete: a completed single-line item renders as
        // markdown strikethrough (`~~x~~`), and round-trips through the parser.
        let entries = vec![QueueEntry::Completed(QueuePrompt {
            text: "do [#x]".to_string(),
            multiline: false,
        })];
        let rendered = render(&entries);
        assert_eq!(rendered, "- ~~do [#x]~~\n");
        // Legacy single-tilde residue still parses back as Completed.
        let reparsed = parse("- ~~do [#x]~~\n").unwrap();
        assert!(matches!(&reparsed[0], QueueEntry::Completed(p) if p.text == "do [#x]"));
        let legacy = parse("- ~do [#x]~\n").unwrap();
        assert!(matches!(&legacy[0], QueueEntry::Completed(p) if p.text == "do [#x]"));
    }

    #[test]
    fn item_after_deps_parses_tokens() {
        assert_eq!(
            agent_doc_core::pending::item_after_deps("do the thing after=#a"),
            vec!["a".to_string()]
        );
        assert_eq!(
            agent_doc_core::pending::item_after_deps("x after=#a,#b more"),
            vec!["a".to_string(), "b".to_string()]
        );
        // word-boundary guard: `hereafter=` must not match.
        assert!(agent_doc_core::pending::item_after_deps("hereafter=#a").is_empty());
    }

    #[test]
    fn pin_tiers_with_asterisk_emphasis() {
        // Operator pin (**strong**) is position-locked at slot 2; agent pin
        // (*italic*) floats above the unpinned prompt in the movable slots.
        let entries = parse(concat!(
            "- do [#a]\n",
            "- *prioritized* do [#b]\n",
            "- **prioritized** do [#c]\n",
        ))
        .unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8);
        let sorted = sort_prompts_by_priority(&entries, &rank, &std::collections::HashSet::new())
            .expect("tiers reorder");
        assert_eq!(
            render(&sorted),
            "- *prioritized* do [#b]\n- do [#a]\n- **prioritized** do [#c]\n"
        );
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
    fn dedup_live_prompts_preserves_free_text_duplicates() {
        let entries = parse(concat!("- do deploy\n", "- do deploy\n")).unwrap();
        let deduped = dedup_live_prompts(&entries);
        assert!(
            deduped.is_none(),
            "free-text duplicate prompts should stay as user intent"
        );
        assert_eq!(
            prompts(&entries).len(),
            2,
            "free-text duplicate prompts are intentionally preserved: {entries:?}"
        );
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
    fn dedup_live_prompts_collapses_pin_prefixed_id_duplicate() {
        // #qdup-pin-prefix: a re-pinned id-backed head (`:pushpin: do [#x]`) must
        // collapse against its unpinned twin (`do [#x]`) — the pin marker is a
        // priority annotation, not part of the queue identity. Before the fix the
        // pin prefix defeated the `do [#` identity check, so the duplicate
        // accumulated visibly in the queue.
        let entries = parse(concat!(
            "- do [#x]\n",
            "- :pushpin: do [#x]\n",
            "- :round_pushpin: Do [#x]\n",
        ))
        .unwrap();
        let deduped =
            dedup_live_prompts(&entries).expect("pin-prefixed id duplicates should collapse");
        assert_eq!(
            prompts(&deduped).len(),
            1,
            "all three same-id heads collapse to the first occurrence: {deduped:?}"
        );
        assert_eq!(render(&deduped), "- do [#x]\n");
    }

    #[test]
    fn dedup_bare_id_reference_heads_collapses_mirror_duplicates() {
        // #qdup-bare-id: the backlog→queue mirror emits a BARE `[#id]` head (no `do`).
        // The operator-reported "agent-doc duplicated my queue items" bug was
        // `[#sqedit-race]` / `[#qpausemix-verify]` appearing twice. Bare reference
        // dups collapse; `do [#id]` directive dups are PRESERVED (intentional intent,
        // #queue-dedup-destroys-intentional-duplicates); free-text is preserved.
        let entries = parse(concat!(
            "- [#sqedit-race]\n",
            "- [#qpausemix-verify]\n",
            "- [#sqedit-race]\n",                    // bare duplicate → collapse
            "- :pushpin: [#qpausemix-verify]\n",     // pinned bare dup → collapse
            "- #sqedit-race\n",                      // unbracketed bare dup → collapse
            "- do [#sqedit-race]\n",                 // `do` directive → PRESERVED
            "- do [#sqedit-race]\n",                 // intentional `do` duplicate → PRESERVED
            "- #sqedit-race continue the drain\n",   // free-text citing an id → PRESERVED
        ))
        .unwrap();
        let deduped = dedup_bare_id_reference_heads(&entries)
            .expect("bare id-reference duplicates should collapse");
        assert_eq!(
            render(&deduped),
            concat!(
                "- [#sqedit-race]\n",
                "- [#qpausemix-verify]\n",
                "- do [#sqedit-race]\n",
                "- do [#sqedit-race]\n",
                "- #sqedit-race continue the drain\n",
            ),
            "bare refs collapse to first; do-directives + free-text preserved: {deduped:?}"
        );
    }

    #[test]
    fn dedup_bare_id_reference_heads_noop_without_bare_duplicates() {
        let entries = parse("- do [#a]\n- do [#a]\n- [#b]\n- #c continue\n").unwrap();
        assert!(
            dedup_bare_id_reference_heads(&entries).is_none(),
            "no bare-ref duplicates (do-dups are intentional) → no mutation"
        );
    }

    #[test]
    fn sync_does_not_readd_id_already_present_as_pinned_head() {
        // The operator's requirement (`agent:backlog` with a `queue` attribute must
        // not re-add an item already present in the queue) holds even when the
        // existing head carries a leading pin marker: `entry_do_id` reads the id
        // from anywhere in the text, so Append/Prepend skip it.
        let entries = parse("- :pushpin: do [#foo]\n").unwrap();
        assert!(
            sync_backlog_into_queue(&entries, &ids(&["foo"]), BacklogQueueSyncMode::Append)
                .is_none(),
            "id already present as a pinned head must not be re-appended"
        );
        assert!(
            sync_backlog_into_queue(&entries, &ids(&["foo"]), BacklogQueueSyncMode::Prepend)
                .is_none(),
            "id already present as a pinned head must not be re-prepended"
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
        assert!(
            rendered.contains("- There is significant blocking"),
            "{rendered}"
        );
        assert!(!rendered.contains("`-"), "{rendered}");
    }

    #[test]
    fn parse_completed_single_line_item() {
        // Legacy single-tilde input parses as Completed; render canonicalizes to
        // double-tilde markdown strikethrough (#queue-strike-on-complete).
        let body = "- ~do #fix1~\n- do #fix2\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(matches!(&entries[0], QueueEntry::Completed(p) if p.text == "do #fix1"));
        assert!(matches!(&entries[1], QueueEntry::Prompt(p) if p.text == "do #fix2"));
        assert_eq!(prompts(&entries).len(), 1);
        assert_eq!(render(&entries), "- ~~do #fix1~~\n- do #fix2\n");
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
    fn parse_bare_slash_command_as_prompt() {
        let body = "\n  /clear  \n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(matches!(&entries[0], QueueEntry::Prompt(p) if p.text == "/clear"));
        assert_eq!(first_prompt(&entries).unwrap().text, "/clear");
        assert_eq!(render(&entries), "/clear\n");
    }

    #[test]
    fn parse_bare_non_slash_line_remains_freeform() {
        let body = "clear the context\n";
        let entries = parse(body).unwrap();
        assert_eq!(
            entries,
            vec![QueueEntry::Freeform("clear the context".to_string())]
        );
        assert!(first_prompt(&entries).is_none());
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
        assert_eq!(render(&result), "preset spec\n- ~~do #fix1~~\n- do #fix2\n");
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
            "dispatch #spec\n- ~~do #fix1~~\n- do #fix2\n"
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
            strip_control_from_tag("<!-- agent:queue preset=\"#p\"=true go=true -->"),
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
        assert_eq!(
            strip_auto_from_tag("<!-- agent:queue auto=true priority=true -->"),
            "<!-- agent:queue priority -->"
        );
    }

    #[test]
    fn strip_auto_from_tag_noop_without_auto() {
        assert_eq!(
            strip_auto_from_tag("<!-- agent:queue -->"),
            "<!-- agent:queue -->"
        );
    }

    #[test]
    fn normalize_queue_tag_attrs_repairs_boolean_true_regression() {
        assert_eq!(
            normalize_queue_tag_attrs(
                "<!-- agent:queue priority=true preset=\"#spec-test-build-install-commit-push\"=true go=true -->"
            ),
            "<!-- agent:queue priority preset=\"#spec-test-build-install-commit-push\" go -->"
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
