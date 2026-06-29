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

use agent_doc_element_queue::QueueItemLifecycle;
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
                entries.push((
                    QueueEntry::Preset(preset.to_string()),
                    span(start_i, start_i + 1),
                ));
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
            entries.push((
                QueueEntry::Freeform(line.to_string()),
                span(start_i, start_i + 1),
            ));
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
            entries.push((
                QueueEntry::Freeform(line.to_string()),
                span(start_i, start_i + 1),
            ));
            i += 1;
            continue;
        }

        // Operator-entered diagnostic queue heads are often typed as prose plus a
        // fenced log followed by a `---` separator rather than wrapped in an
        // opening `---` fence. Treat that block as one multiline prompt and
        // consume the separator with it so closeout can strike/consume the exact
        // visible head (#qfreetext-sep). Keep ordinary malformed prose without a
        // diagnostic fence as Freeform so legacy fixed-point cleanup does not
        // promote stray notes into runnable prompts.
        if let Some(close_idx) = (i + 1..lines.len()).find(|&j| lines[j].trim() == "---") {
            let text = lines[i..close_idx].join("\n");
            if !text.trim().is_empty() && (text.contains("```") || text.contains("~~~")) {
                entries.push((
                    QueueEntry::Prompt(QueuePrompt {
                        text,
                        multiline: true,
                    }),
                    span(start_i, close_idx + 1),
                ));
                i = close_idx + 1;
                continue;
            }
        }

        // Unrecognized line: preserve verbatim instead of failing the parse so
        // queue consume/resume/dispatch guards stay resilient to a polluted
        // queue body (#jb-run-agent-doc-response-queue-contamination). The line
        // is preserved as-is and never treated as an actionable item.
        entries.push((
            QueueEntry::Freeform(line.to_string()),
            span(start_i, start_i + 1),
        ));
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
///   `do [#id]` list, in backlog order, while preserving no-id manual queue
///   entries (free-text prompts, fences, prose). Stale id-backed mirror entries
///   are dropped. Use this when the id-backed queue should mirror the backlog
///   each cycle without deleting realtime operator-authored queue text.
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

/// All backlog `#id`s referenced on a single queue entry's text. A manually
/// authored multi-id line (`[#a] [#b] [#c]`) references several ids; the
/// backlog→queue mirror must treat **every** one of them as already-present so
/// `sync_backlog_into_queue` does not re-add the trailing ids as duplicate
/// `do [#id]` lines (#provauth2 — operator-authored multi-id lines must not be
/// duplicated by the auto-add path). `do_prompt_id` (singular) only sees the
/// first `#id`, which is the dedup gap that proliferated duplicates.
fn do_prompt_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = text;
    while let Some(marker) = rest.find('#') {
        let after = &rest[marker + 1..];
        let id: String = after
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
            .collect();
        if !id.is_empty() {
            ids.push(id.to_ascii_lowercase());
        }
        rest = after;
    }
    ids
}

/// Every backlog `#id` referenced on a queue entry (multi-id aware). See
/// [`do_prompt_ids`] (#provauth2).
fn entry_do_ids(entry: &QueueEntry) -> Vec<String> {
    match entry {
        QueueEntry::Prompt(p) | QueueEntry::Completed(p) => do_prompt_ids(&p.text),
        _ => Vec::new(),
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

    // Multi-id aware (#provauth2): a manually-authored `[#a] [#b] [#c]` line
    // marks a, b AND c as present so the mirror never re-adds the trailing ids
    // as duplicate `do [#id]` lines.
    let existing_ids: std::collections::HashSet<String> =
        entries.iter().flat_map(entry_do_ids).collect();

    let new_entries: Vec<QueueEntry> = match mode {
        BacklogQueueSyncMode::Sync => {
            // Mirror backlog-backed directives, but never delete no-id operator
            // queue text such as a freshly typed `- test`.
            let mirror = ordered_ids
                .iter()
                .map(|id| do_prompt_entry(id))
                .collect::<Vec<_>>();
            let mut rebuilt = Vec::with_capacity(entries.len().max(mirror.len()));
            let mut inserted_mirror = false;
            for entry in entries {
                if entry_do_ids(entry).is_empty() {
                    rebuilt.push(entry.clone());
                } else if !inserted_mirror {
                    rebuilt.extend(mirror.iter().cloned());
                    inserted_mirror = true;
                }
            }
            if !inserted_mirror {
                rebuilt.extend(mirror);
            }
            rebuilt
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
pub use agent_doc_document::queue_projection::{
    AGENT_PRIORITIZED_MARKER, AGENT_PRIORITIZED_MARKERS, IN_PROGRESS_MARKER, PRIORITIZED_MARKER,
    PRIORITIZED_MARKERS, apply_in_progress_marker, has_in_progress_marker,
    strip_in_progress_marker, strip_in_progress_marker_for_display, strip_priority_markers,
};

/// True when `text` carries an **operator** (strong-emphasis) pin marker at its
/// head (after optional leading whitespace), in either spelling.
pub fn is_prioritized(text: &str) -> bool {
    let stripped = strip_in_progress_marker(text);
    let t = stripped.trim_start();
    PRIORITIZED_MARKERS.iter().any(|m| t.starts_with(m))
}

/// True when `text` carries an **agent** (italic-emphasis) pin marker at its
/// head but is not an operator pin. A strong-emphasis wrap (`**x**` / `__x__`)
/// never starts with its italic counterpart (`*x*` / `_x_`) — second char
/// differs — so the tiers are cleanly separable; the operator-pin guard is
/// belt-and-suspenders.
pub fn is_agent_prioritized(text: &str) -> bool {
    let stripped = strip_in_progress_marker(text);
    let t = stripped.trim_start();
    !is_prioritized(text) && AGENT_PRIORITIZED_MARKERS.iter().any(|m| t.starts_with(m))
}

/// Clear all queue in-progress markers, then mark the first live prompt when
/// requested. Returns `None` when the rendered queue would be unchanged.
pub fn set_first_prompt_in_progress(
    entries: &[QueueEntry],
    mark_first_prompt: bool,
) -> Option<Vec<QueueEntry>> {
    let target_texts = if mark_first_prompt {
        prompts(entries)
            .first()
            .map(|prompt| vec![strip_in_progress_marker(&prompt.text)])
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    set_prompts_in_progress(entries, &target_texts)
}

/// Clear all queue in-progress markers, then mark the requested live prompt(s).
///
/// The visible `🚧` marker is a realtime projection of the active head set, not
/// prompt identity. The target list is a multiset: duplicate target texts mark
/// the same number of matching prompts, which leaves room for concurrent active
/// heads while avoiding accidental duplicate marking from repeated queue rows.
pub fn set_prompts_in_progress(
    entries: &[QueueEntry],
    target_texts: &[String],
) -> Option<Vec<QueueEntry>> {
    let mut target_counts = std::collections::BTreeMap::<String, usize>::new();
    for text in target_texts {
        let key = strip_priority_markers(text);
        if key.is_empty() {
            continue;
        }
        *target_counts.entry(key).or_default() += 1;
    }
    let mut changed = false;
    let out = entries
        .iter()
        .map(|entry| match entry {
            QueueEntry::Prompt(prompt) => {
                let mut next = prompt.clone();
                if let Some(stripped) = strip_in_progress_marker_for_display(&next.text) {
                    next.text = stripped;
                    changed = true;
                }
                let key = strip_priority_markers(&next.text);
                if let Some(remaining) = target_counts.get_mut(&key)
                    && *remaining > 0
                {
                    *remaining -= 1;
                    if !crate::queue_command::is_slash_command(&next.text) {
                        let marked_text = apply_in_progress_marker(&next.text);
                        if marked_text != next.text {
                            next.text = marked_text;
                            changed = true;
                        }
                    }
                }
                QueueEntry::Prompt(next)
            }
            QueueEntry::Completed(prompt) => {
                let mut next = prompt.clone();
                if let Some(stripped) = strip_in_progress_marker_for_display(&next.text) {
                    next.text = stripped;
                    changed = true;
                }
                QueueEntry::Completed(next)
            }
            _ => entry.clone(),
        })
        .collect::<Vec<_>>();
    changed.then_some(out)
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

/// Stable prompt identities for freshly operator-added queue prompts (`#7r2s`).
///
/// `annotate_operator_priority_reorders` only handles an *existing* prompt that
/// moved earlier. A brand-new line the operator just typed into the queue is
/// absent from `snapshot`; when it is also not one the binary just appended from
/// the backlog this cycle (`synced_ids`), treat its pin-independent text as an
/// operator-authored identity. Priority/DAG sorting can then position-lock that
/// identity at its authored slot without mutating visible prompt text with a
/// synthetic `:pushpin:`.
pub fn operator_authored_prompt_identities(
    snapshot: &[QueueEntry],
    current: &[QueueEntry],
    synced_ids: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
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
    let mut identities = std::collections::HashSet::new();
    for entry in current {
        let do_id = entry_do_id(entry);
        let QueueEntry::Prompt(prompt) = entry else {
            continue;
        };
        let identity = strip_priority_markers(&prompt.text);
        if identity.trim().is_empty() {
            continue;
        }
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
            continue; // already carries an explicit priority marker
        }
        identities.insert(identity);
    }
    identities
}

/// Auto-pin freshly operator-added queue prompts with the operator priority
/// marker (`#7r2s`).
///
/// Kept as a compatibility shim for older call sites/tests. New operator-added
/// prompts are now position-locked by passing [`operator_authored_prompt_identities`]
/// to the priority/DAG sort, which avoids injecting a visible `:pushpin:`.
pub fn annotate_manual_queue_additions(
    snapshot: &[QueueEntry],
    current: &[QueueEntry],
    synced_ids: &std::collections::HashSet<String>,
) -> Option<Vec<QueueEntry>> {
    let _ = operator_authored_prompt_identities(snapshot, current, synced_ids);
    None
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

fn entry_identity(entry: &QueueEntry) -> Option<String> {
    match entry {
        QueueEntry::Prompt(p) | QueueEntry::Completed(p) => Some(strip_priority_markers(&p.text)),
        _ => None,
    }
}

fn entry_is_operator_authored(
    entry: &QueueEntry,
    operator_authored: &std::collections::HashSet<String>,
) -> bool {
    !operator_authored.is_empty()
        && entry_identity(entry).is_some_and(|identity| operator_authored.contains(&identity))
}

pub fn sort_prompts_by_priority(
    entries: &[QueueEntry],
    rank: &std::collections::HashMap<String, u8>,
    backlog_sourced: &std::collections::HashSet<String>,
) -> Option<Vec<QueueEntry>> {
    sort_prompts_by_priority_with_operator_authored(
        entries,
        rank,
        backlog_sourced,
        &std::collections::HashSet::new(),
    )
}

pub fn sort_prompts_by_priority_with_operator_authored(
    entries: &[QueueEntry],
    rank: &std::collections::HashMap<String, u8>,
    backlog_sourced: &std::collections::HashSet<String>,
    operator_authored: &std::collections::HashSet<String>,
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
    // Anchored slots are position-locked: operator pins (tier 0),
    // operator-authored identities (`#qauthorderpin`), and free-text operator
    // lines (`#qauthorder`). Only the remaining movable prompts reorder, filling
    // the slots not held by an anchor.
    let is_anchored = |idx: usize| {
        entry_priority_tier(&prompts[idx]) == 0
            || entry_is_operator_authored(&prompts[idx], operator_authored)
            || is_free_text_prompt(&prompts[idx])
    };
    let mut movable: Vec<usize> = (0..n).filter(|&i| !is_anchored(i)).collect();
    movable.sort_by_key(|&i| key(i));
    // Reassemble: walk the prompt slots in document order. An anchored slot keeps
    // its own prompt; every other slot draws the next entry from the reordered
    // `movable` queue.
    let mut mv = movable.into_iter();
    let mut order: Vec<usize> = Vec::with_capacity(n);
    for (i, _prompt) in prompts.iter().enumerate() {
        if is_anchored(i) {
            order.push(i);
        } else {
            order.push(
                mv.next()
                    .expect("movable slot count matches non-anchored prompts"),
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
    sort_prompts_by_dag_with_operator_authored(
        entries,
        rank,
        deps,
        backlog_sourced,
        &std::collections::HashSet::new(),
    )
}

pub fn sort_prompts_by_dag_with_operator_authored(
    entries: &[QueueEntry],
    rank: &std::collections::HashMap<String, u8>,
    deps: &std::collections::HashMap<String, Vec<String>>,
    backlog_sourced: &std::collections::HashSet<String>,
    operator_authored: &std::collections::HashSet<String>,
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
    // Anchored slots are position-locked in the DAG order: operator pins (tier 0),
    // operator-authored identities (`#qauthorderpin`), and free-text operator
    // lines (`#qauthorder`). Only movable prompts reorder around them, in
    // dependency-respecting priority order.
    let is_anchored = |idx: usize| {
        entry_priority_tier(&prompts[idx]) == 0
            || entry_is_operator_authored(&prompts[idx], operator_authored)
            || is_free_text_prompt(&prompts[idx])
    };

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
    let order: Vec<usize> = if !(0..n).any(is_anchored) {
        plain_order()
    } else {
        let movable: Vec<usize> = (0..n).filter(|&i| !is_anchored(i)).collect();
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
        // Fill: anchored slots (operator pins + free-text) keep their document
        // slot; movable prompts fill the rest in `movable_seq` order.
        let mut anchored: Vec<usize> = Vec::with_capacity(n);
        let mut mv = movable_seq.into_iter();
        for slot in 0..n {
            if is_anchored(slot) {
                anchored.push(slot);
            } else {
                anchored.push(
                    mv.next()
                        .expect("movable prompts fill every non-anchored slot"),
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
    let inner = parse_completed_inline_once(trimmed)?;
    let inner = parse_completed_inline_once(inner).unwrap_or(inner);
    let inner = inner.trim();
    (!inner.is_empty()).then_some(inner)
}

fn parse_completed_inline_once(trimmed: &str) -> Option<&str> {
    if let Some(inner) = trimmed
        .strip_prefix("~~")
        .and_then(|s| s.strip_suffix("~~"))
    {
        let inner = inner.trim();
        return (!inner.is_empty()).then_some(inner);
    }
    if let Some(inner) = trimmed.strip_prefix('~').and_then(|s| s.strip_suffix('~')) {
        let inner = inner.trim();
        return (!inner.is_empty()).then_some(inner);
    }
    let rest = trimmed.strip_prefix("~~")?;
    let needle = format!(
        "~~{}",
        agent_doc_markdown_ast::overlay::STRUCK_ANNOTATION_SEPARATOR
    );
    let close = rest.find(&needle)?;
    let inner = rest[..close].trim();
    (!inner.is_empty()).then_some(inner)
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
    let trimmed = strip_priority_markers(&prompt.text)
        .trim()
        .to_ascii_lowercase();
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

/// Priority rank of a queue head's leading pin marker: operator pin
/// (`:pushpin:`) = 2 (highest) > agent pin (`:round_pushpin:`) = 1 > bare = 0.
/// Used to choose the surviving instance when collapsing pin-variant duplicates
/// of the same id (#qdedupsync).
fn do_head_priority_rank(text: &str) -> u8 {
    if is_prioritized(text) {
        2
    } else if is_agent_prioritized(text) {
        1
    } else {
        0
    }
}

/// Collapse same-id `do [#id]` / `do #id` directive heads that disagree **only**
/// on their leading priority marker — a `:pushpin:`-pinned head that accumulated
/// ALONGSIDE its bare twin (or two different marker spellings of the same id) —
/// into a single highest-priority instance kept at the group's earliest position
/// (#qdedupsync / #pushpinaccum).
///
/// The backlog→queue mirror and CRDT replay both emit the bare `do [#id]` form;
/// an operator/agent pin adds a *pinned copy* of the same id instead of replacing
/// the bare one, so the queue accumulates `:pushpin: do [#id]` AND `do [#id]`
/// (live repro: this very document held both `:pushpin: do [#6b5hwire]` and a
/// bare `do [#6b5hwire]`). Unlike [`dedup_live_prompts`], this deliberately
/// PRESERVES **textually identical** `do [#id]` duplicates — two bare or two
/// identically-pinned heads can be intentional "run it twice" queue intent
/// (`#queue-dedup-destroys-intentional-duplicates`). Only a same-id group whose
/// members are not all identical (they disagree on the pin prefix) is collapsed,
/// and the surviving entry keeps the strongest pin observed for that id.
/// `Completed`/`Preset`/fence entries are left untouched.
pub fn dedup_pin_variant_do_heads(entries: &[QueueEntry]) -> Option<Vec<QueueEntry>> {
    // Group prompt indices by their normalized `do [#id]` identity, preserving
    // first-seen order. `dedup_key_for_prompt` returns `Some(id)` only for
    // `do [#id]`/`do #id` heads (after stripping pins + lowercasing), so free-text
    // heads are never grouped.
    let mut groups: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for (idx, entry) in entries.iter().enumerate() {
        if let QueueEntry::Prompt(prompt) = entry
            && let Some(id) = dedup_key_for_prompt(prompt)
        {
            groups
                .entry(id.clone())
                .or_insert_with(|| {
                    order.push(id);
                    Vec::new()
                })
                .push(idx);
        }
    }

    let prompt_text = |idx: usize| match &entries[idx] {
        QueueEntry::Prompt(p) => p.text.as_str(),
        _ => unreachable!("grouped index always points at a Prompt entry"),
    };
    let logical_prompt_text = |idx: usize| strip_in_progress_marker(prompt_text(idx));

    // For each id group with >1 member that are NOT all textually identical:
    // keep the group's earliest position, but overwrite its text with the
    // strongest-pin variant (highest rank, earliest on tie); drop the rest.
    let mut drop: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut keep_text: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
    for id in &order {
        let idxs = &groups[id];
        if idxs.len() < 2 {
            continue;
        }
        let first_text = logical_prompt_text(idxs[0]);
        if idxs.iter().all(|&i| logical_prompt_text(i) == first_text) {
            // Intentional "run it twice" — identical heads, leave untouched.
            continue;
        }
        let earliest = idxs[0];
        let best = *idxs
            .iter()
            .max_by(|&&a, &&b| {
                do_head_priority_rank(prompt_text(a))
                    .cmp(&do_head_priority_rank(prompt_text(b)))
                    .then_with(|| b.cmp(&a))
            })
            .expect("group is non-empty");
        keep_text.insert(earliest, prompt_text(best).to_string());
        for &i in idxs {
            if i != earliest {
                drop.insert(i);
            }
        }
    }

    if drop.is_empty() {
        return None;
    }
    let deduped: Vec<QueueEntry> = entries
        .iter()
        .enumerate()
        .filter(|(i, _)| !drop.contains(i))
        .map(|(i, entry)| match keep_text.get(&i) {
            Some(text) if matches!(entry, QueueEntry::Prompt(_)) => {
                QueueEntry::Prompt(QueuePrompt {
                    text: text.clone(),
                    multiline: false,
                })
            }
            _ => entry.clone(),
        })
        .collect();
    Some(deduped)
}

/// True when `entry` is a single-line **free-text operator prompt** — a `Prompt`
/// that is neither a `do [#id]`/`do #id` directive head nor a pure `[#id]`/`#id`
/// reference head (`#qauthorder`).
///
/// Free-text queue lines are only ever authored by the operator (the binary
/// emits `do [#id]` heads from the backlog, never prose), so they carry an
/// implicit position-lock: the priority/DAG sort anchors them at their authored
/// document slot — exactly like an operator pin (tier 0) — instead of bubbling
/// them to the top or sinking them under `queue`-attr backlog items. This holds
/// the slot WITHOUT mutating the line's visible text (no injected `:pushpin:`),
/// which is the mechanism the operator-authored-order convention requires.
/// Multiline blocks are excluded (their reordering semantics are unchanged).
fn is_free_text_prompt(entry: &QueueEntry) -> bool {
    match entry {
        QueueEntry::Prompt(p) => {
            !p.multiline && dedup_key_for_prompt(p).is_none() && bare_id_reference_key(p).is_none()
        }
        _ => false,
    }
}

/// Dedup key for a free-text operator queue line, or `None` when the entry is
/// not a single-line free-text prompt (`#qauthorder`).
///
/// The tuple's first element distinguishes live (`0`) from struck/`Completed`
/// (`1`) lines so a genuine "in progress" + "done" pair of the same text is
/// preserved; id-backed heads (`do [#id]` / pure `[#id]`) return `None` because
/// they are collapsed by the id-keyed dedups (`dedup_pin_variant_do_heads`,
/// `dedup_bare_id_reference_heads`). The cosmetic pin marker is stripped and the
/// text lowercased so a re-emit that only differs by an injected pin or case
/// still collapses.
fn free_text_dedup_key(entry: &QueueEntry) -> Option<(u8, String)> {
    let (variant, prompt) = match entry {
        QueueEntry::Prompt(p) => (0u8, p),
        QueueEntry::Completed(p) => (1u8, p),
        _ => return None,
    };
    if dedup_key_for_prompt(prompt).is_some() || bare_id_reference_key(prompt).is_some() {
        return None; // id-backed head — handled by the id-keyed dedups
    }
    let normalized = strip_priority_markers(&prompt.text);
    if normalized.is_empty() {
        return None;
    }
    // Multiline phantom pins (e.g. a `:round_pushpin:` actor-switch paste block
    // with a fenced route-error body) re-emit verbatim under a stale-CRDT /
    // supervisor convergence, flooding the queue with near-identical copies
    // (`#rt83qflood`). Key them on whitespace-collapsed lowercased text so an
    // exact re-paste — which may differ only in trailing/blank-line whitespace —
    // keys identically and collapses to its snapshot-authored multiplicity, just
    // like the single-line free-text case. Single-line keys keep their exact
    // normalization to avoid regressing the existing behavior.
    let key = if prompt.multiline {
        normalize_multiline_dedup_text(&normalized)
    } else {
        normalized.to_ascii_lowercase()
    };
    Some((variant, key))
}

/// Whitespace-collapsed, lowercased dedup key for a multiline free-text queue
/// pin so verbatim re-emits (differing only in insignificant whitespace) key
/// identically (`#rt83qflood`).
pub(crate) fn normalize_multiline_dedup_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// Collapse duplicate **free-text** operator queue lines that convergence
/// re-emitted beyond the authored count, keeping the earliest occurrences
/// (`#qauthorder`).
///
/// The id-keyed dedups above (`dedup_bare_id_reference_heads`,
/// `dedup_pin_variant_do_heads`) are free-text-blind, so a CRDT/backlog-sync
/// re-emit of an operator's free-text line accumulated as a visible duplicate
/// (live repro: an operator's "queue items ... not automatically bubbled to the
/// top" line emitted twice in a single preflight, once struck).
///
/// The discriminator between an intentional "run it twice" duplicate
/// (`#queue-dedup-destroys-intentional-duplicates`) and a convergence artifact
/// is the **snapshot**: `snapshot_entries` is the committed, operator-authored
/// queue. A free-text line may appear up to as many times as it does in the
/// snapshot (at least once is always allowed); any copies BEYOND that authored
/// multiplicity are a convergence artifact and are dropped. So two committed
/// `do deploy` lines stay two, but a single committed line re-emitted twice
/// collapses back to one. Lines are keyed after stripping the cosmetic pin
/// marker (case-insensitively) so a pin-injected re-emit still collapses; live
/// and struck (`Completed`) lines are keyed separately here so a genuine
/// in-progress vs done pair survives; the later unified lifecycle convergence
/// still lets struck snapshot evidence dominate stale live re-emits. Multiline
/// free-text pins (e.g. `:round_pushpin:` actor-switch paste blocks) are deduped
/// too, keyed on their whitespace-collapsed text so a verbatim phantom re-emit
/// converges to the authored count (`#rt83qflood`). `Preset`/`Dispatch`/fence/
/// `Freeform` entries and id-backed heads are left untouched.
pub fn dedup_free_text_heads(
    entries: &[QueueEntry],
    snapshot_entries: &[QueueEntry],
) -> Option<Vec<QueueEntry>> {
    // Authored multiplicity per free-text key (how many copies the operator
    // committed). A line may legitimately appear that many times; at least one
    // copy is always allowed even for a line absent from the snapshot.
    let mut authored: std::collections::HashMap<(u8, String), usize> =
        std::collections::HashMap::new();
    for entry in snapshot_entries {
        if let Some(key) = free_text_dedup_key(entry) {
            *authored.entry(key).or_insert(0) += 1;
        }
    }
    let mut kept: std::collections::HashMap<(u8, String), usize> = std::collections::HashMap::new();
    let mut deduped = Vec::with_capacity(entries.len());
    let mut dropped = false;
    for entry in entries {
        if let Some(key) = free_text_dedup_key(entry) {
            let allowed = authored.get(&key).copied().unwrap_or(0).max(1);
            let seen = kept.entry(key).or_insert(0);
            if *seen >= allowed {
                dropped = true;
                continue;
            }
            *seen += 1;
        }
        deduped.push(entry.clone());
    }
    if dropped { Some(deduped) } else { None }
}

/// The visible lifecycle of a queue entry, projected onto the
/// [`QueueItemLifecycle`] lattice the per-item state machine joins over
/// (`#queuestatemachine2` / `#cgfx`). A `Prompt` is a `Live` head; a `Completed`
/// (struck) entry is the retired `Struck` state. Non-prompt entries have no
/// lifecycle.
fn entry_lifecycle(entry: &QueueEntry) -> Option<QueueItemLifecycle> {
    match entry {
        QueueEntry::Prompt(_) => Some(QueueItemLifecycle::Live),
        QueueEntry::Completed(_) => Some(QueueItemLifecycle::Struck),
        _ => None,
    }
}

/// The structural shape of a prompt-bearing queue head, the discriminator the
/// historical passes used to decide whether a same-identity duplicate is a
/// convergence artifact or intentional authoring. Folded into the convergence
/// key so each shape keeps its own collapse rule and a `do [#id]` directive twin
/// never shares a key with a bare `[#id]` reference of the same id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum HeadShape {
    /// `do [#id]` / `do #id` directive head. Identical twins are intentional
    /// "run it twice"; only pin-variant disagreement collapses
    /// (`dedup_pin_variant_do_heads` / `#qdedupsync`).
    Directive,
    /// Pure `[#id]` / `#id` reference head — always a mirror/replay artifact when
    /// duplicated (`dedup_bare_id_reference_heads` / `#qdup-bare-id`).
    BareReference,
    /// Free-text operator line (`#qauthorder` / `#rt83qflood`) — collapses beyond
    /// snapshot-authored multiplicity.
    FreeText,
}

fn head_shape(prompt: &QueuePrompt) -> HeadShape {
    if dedup_key_for_prompt(prompt).is_some() {
        HeadShape::Directive
    } else if bare_id_reference_key(prompt).is_some() {
        HeadShape::BareReference
    } else {
        HeadShape::FreeText
    }
}

/// Full convergence key: durable identity, head shape, and visible lifecycle.
///
/// - **Identity** is exactly [`QueueItemIdentity::from_prompt`] (id-backed heads
///   collapse every pin/prefix variant onto the durable id; free-text keys on
///   normalized text) — the single dedup key the four historical ad-hoc passes
///   each re-derived a partial version of.
/// - **Shape** keeps each head shape's collapse rule independent, matching the
///   original pass separation (directive twins are intentional; bare references
///   and free-text re-emits collapse).
/// - **Lifecycle** partitions live vs struck so a genuine "in progress" + "done"
///   pair of the same work survives, while snapshot/current struck evidence still
///   dominates stale live re-emits for the same identity.
///
/// Returns `None` for non-prompt entries (fences, presets, dispatch, freeform),
/// which convergence leaves untouched.
type ConvergenceKey = (
    agent_doc_element_queue::QueueItemIdentity,
    HeadShape,
    QueueItemLifecycle,
);

fn convergence_identity(entry: &QueueEntry) -> Option<ConvergenceKey> {
    let prompt = match entry {
        QueueEntry::Prompt(p) | QueueEntry::Completed(p) => p,
        _ => return None,
    };
    let lifecycle = entry_lifecycle(entry)?;
    let identity = agent_doc_element_queue::QueueItemIdentity::from_prompt(&prompt.text);
    Some((identity, head_shape(prompt), lifecycle))
}

fn queue_item_identity(entry: &QueueEntry) -> Option<agent_doc_element_queue::QueueItemIdentity> {
    let prompt = match entry {
        QueueEntry::Prompt(p) | QueueEntry::Completed(p) => p,
        _ => return None,
    };
    Some(agent_doc_element_queue::QueueItemIdentity::from_prompt(
        &prompt.text,
    ))
}

fn snapshot_struck_representatives(
    snapshot_entries: &[QueueEntry],
) -> std::collections::HashMap<agent_doc_element_queue::QueueItemIdentity, QueueEntry> {
    let mut struck = std::collections::HashMap::new();
    for entry in snapshot_entries {
        if !matches!(entry, QueueEntry::Completed(_)) {
            continue;
        }
        if let Some(identity) = queue_item_identity(entry) {
            struck.entry(identity).or_insert_with(|| entry.clone());
        }
    }
    struck
}

fn current_struck_identities(
    entries: &[QueueEntry],
) -> std::collections::HashSet<agent_doc_element_queue::QueueItemIdentity> {
    entries
        .iter()
        .filter(|entry| matches!(entry, QueueEntry::Completed(_)))
        .filter_map(queue_item_identity)
        .collect()
}

/// `true` when two same-identity `do [#id]` directive heads are *textually
/// identical* (modulo the in-progress marker) and therefore an intentional
/// "run it twice" queue authoring (`#queue-dedup-destroys-intentional-duplicates`),
/// not a convergence artifact. Free-text and bare-`[#id]` reference heads are
/// never intentional duplicates — a repeated prose line / bare reference is
/// always a mirror/replay artifact — so this only guards the `do [#id]` shape.
fn is_intentional_directive_twin(a: &QueuePrompt, b: &QueuePrompt) -> bool {
    let a_id = dedup_key_for_prompt(a);
    let b_id = dedup_key_for_prompt(b);
    match (a_id, b_id) {
        (Some(ai), Some(bi)) if ai == bi => {
            strip_in_progress_marker(&a.text) == strip_in_progress_marker(&b.text)
        }
        _ => false,
    }
}

/// Strongest pin marker observed across a group of same-identity `do [#id]`
/// heads, used to choose the survivor's rendered text when collapsing
/// pin-variant duplicates (`#qdedupsync` / `#pushpinaccum`). Operator pin beats
/// agent pin beats bare; ties keep the earliest member's text.
fn strongest_pin_text<'a>(texts: &[&'a str]) -> &'a str {
    texts
        .iter()
        .enumerate()
        .max_by(|(ia, a), (ib, b)| {
            do_head_priority_rank(a)
                .cmp(&do_head_priority_rank(b))
                .then_with(|| ib.cmp(ia))
        })
        .map(|(_, t)| *t)
        .expect("group is non-empty")
}

/// **Unified, state-machine-driven queue convergence (`#queuestatemachine2` /
/// `#cgfx`).**
///
/// This is the single authoritative convergence pass that replaces the pile of
/// independent best-effort dedup normalizers
/// (`dedup_bare_id_reference_heads`, `dedup_pin_variant_do_heads`,
/// `dedup_free_text_heads`, `dedup_live_prompts`) — each of which patched one
/// duplication shape after the fact. Instead of "append then dedup," it drives
/// each **durable head identity** ([`QueueItemIdentity`]) to its lawful state:
/// re-injecting an identity that already has a lawful representative in this
/// convergence is a **no-op transition**, so a stale-CRDT / supervisor re-emit
/// cannot create a visible duplicate. Duplication is structurally impossible,
/// not patched.
///
/// ## How each historical pass becomes a transition guard
///
/// - **bare-id mirror dups** (`#qdup-bare-id` / `dedup_bare_id_reference_heads`)
///   and **free-text re-emit** (`#qauthorder` / `dedup_free_text_heads`,
///   including the `#rt83qflood` multiline phantom-pin flood): a re-sighting of
///   an identity already kept *beyond its authored multiplicity* is a hold (no
///   advance) → dropped. The authored multiplicity comes from `snapshot_entries`
///   exactly as `dedup_free_text_heads` computed it, so an intentionally
///   double-authored line keeps its count.
/// - **pin-variant dups** (`#qdedupsync` / `#pushpinaccum` /
///   `dedup_pin_variant_do_heads`): a same-id `do [#id]` group whose members
///   disagree on the pin prefix collapses to one survivor at the earliest slot,
///   rendered with the strongest pin observed — *unless* the members are
///   textually identical, which stays as intentional "run it twice."
/// - **live-prompt id dups** (`dedup_live_prompts`): subsumed — an id-backed
///   identity beyond its authored multiplicity holds and is dropped.
///
/// ## Preserved invariants
///
/// - **Operator-authored position-lock** (`#queue-operator-pin-position-lock` /
///   `#qauthorder`): convergence never reorders. The survivor of each identity
///   group is kept at the group's **earliest** position, and every non-prompt
///   entry stays in place, so `sort_prompts_by_priority`'s later position-lock
///   sees the same slots. This convergence is purely subtractive (plus the
///   pin-text rewrite on the survivor); it is the `Authored` state's
///   position property carried across the dedup.
/// - **Intentional duplicates** (`#queue-dedup-destroys-intentional-duplicates`):
///   identical `do [#id]` directive twins survive; only re-emit artifacts collapse.
/// - **Live vs struck**: a genuine live + struck pair can survive when the
///   current queue authored both, but terminal evidence from the snapshot/current
///   queue is monotonic (`Live < Struck`). A stale live re-emit of an already
///   struck identity is rewritten to the struck representative (or dropped when a
///   struck representative is already present), so a stale editor buffer cannot
///   resurrect a retired queue head.
///
/// Returns `Some(converged)` when anything changed, `None` when the queue is
/// already at its lawful fixpoint (idempotent — safe every cycle). Driving the
/// output through this function again is a guaranteed no-op (the idempotence
/// property `#cgfx` proves).
pub fn converge_queue_via_lifecycle(
    entries: &[QueueEntry],
    snapshot_entries: &[QueueEntry],
) -> Option<Vec<QueueEntry>> {
    use agent_doc_element_queue::{QueueItemEvent, QueueItemMachine, QueueItemState};

    let snapshot_struck = snapshot_struck_representatives(snapshot_entries);
    let current_struck = current_struck_identities(entries);
    let mut injected_snapshot_struck =
        std::collections::HashSet::<agent_doc_element_queue::QueueItemIdentity>::new();
    let mut lifecycle_changed = false;
    let mut entries_for_convergence = Vec::with_capacity(entries.len());
    for entry in entries {
        if let QueueEntry::Prompt(_) = entry
            && let Some(identity) = queue_item_identity(entry)
            && let Some(struck_entry) = snapshot_struck.get(&identity)
        {
            lifecycle_changed = true;
            if current_struck.contains(&identity) {
                // A current struck copy already carries the terminal state; the
                // live copy is a stale re-emit and should disappear.
                continue;
            }
            if injected_snapshot_struck.insert(identity) {
                entries_for_convergence.push(struck_entry.clone());
            }
            // Additional live copies of the same struck identity are stale
            // re-emits too; one struck representative is enough.
            continue;
        }
        entries_for_convergence.push(entry.clone());
    }
    let entries = entries_for_convergence;

    // Authored multiplicity per (identity, lifecycle): how many copies the
    // operator committed in the snapshot. A given identity may lawfully appear
    // that many times; at least one copy is always allowed even for an identity
    // absent from the snapshot (a freshly-authored line / freshly-mirrored head).
    // This is exactly `dedup_free_text_heads`'s snapshot-authored-count guard,
    // now applied uniformly to every identity shape.
    let mut authored: std::collections::HashMap<_, usize> = std::collections::HashMap::new();
    for entry in snapshot_entries {
        if let Some(key) = convergence_identity(entry) {
            *authored.entry(key).or_insert(0) += 1;
        }
    }

    // First pass: group same-identity prompt indices to detect pin-variant
    // `do [#id]` groups whose survivor needs a text rewrite. Free-text and bare
    // reference identities never rewrite text; only id-backed directive groups do.
    let mut id_groups: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    for (idx, entry) in entries.iter().enumerate() {
        if let QueueEntry::Prompt(p) = entry
            && let Some(id) = dedup_key_for_prompt(p)
        {
            id_groups.entry(id).or_default().push(idx);
        }
    }
    // For each id group with >1 member that are NOT all identical: the survivor
    // (earliest member) should render the strongest-pin variant. Maps the
    // earliest index → rewritten text. Identical twins are left untouched
    // (intentional "run it twice"). A pin-variant group is force-collapsed to a
    // single survivor (its identity's allowed multiplicity is capped at 1)
    // regardless of snapshot count — a pin disagreement is always the
    // accumulation artifact `dedup_pin_variant_do_heads` targeted, never
    // intentional.
    let mut survivor_text: std::collections::HashMap<usize, String> =
        std::collections::HashMap::new();
    let mut pin_collapse_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (id, idxs) in &id_groups {
        if idxs.len() < 2 {
            continue;
        }
        let prompt_at = |i: usize| match &entries[i] {
            QueueEntry::Prompt(p) => p,
            _ => unreachable!("id group index points at a Prompt"),
        };
        let first = prompt_at(idxs[0]);
        if idxs
            .iter()
            .all(|&i| is_intentional_directive_twin(first, prompt_at(i)))
        {
            continue; // all identical — intentional duplicate, never collapse
        }
        let texts: Vec<&str> = idxs.iter().map(|&i| prompt_at(i).text.as_str()).collect();
        survivor_text.insert(idxs[0], strongest_pin_text(&texts).to_string());
        pin_collapse_ids.insert(id.clone());
    }

    // Cross-shape subsumption (`#brtc` / `#queuestatemachine3`): an actionable
    // live `do [#id]` **directive** head subsumes a pure bare `[#id]` / `#id`
    // **reference** of the SAME id — the bare reference is always a mirror/replay
    // artifact (`#qdup-bare-id`) and adds nothing once the directive that does the
    // work is present. The legacy `dedup_bare_id_reference_heads` only collapsed
    // bare references against other bare references, so a directive + reference of
    // one id used to coexist; unifying by *identity* (the whole point of `#cgfx`)
    // means the reference collapses into the directive. This is what lets a
    // re-emit storm converge to exactly ONE visible item per identity.
    let directive_ids: std::collections::HashSet<String> = entries
        .iter()
        .filter_map(|entry| match entry {
            QueueEntry::Prompt(p) => dedup_key_for_prompt(p),
            _ => None,
        })
        .collect();

    // Second pass: drive the lifecycle SM per identity, dropping re-emits beyond
    // the authored multiplicity. The SM makes the no-op explicit — re-sighting an
    // already-kept identity is `OperatorAuthored`/`BacklogMirrored` against an
    // already-advanced state, which holds (no advance), so the copy is dropped.
    let mut machines: std::collections::HashMap<_, QueueItemMachine> =
        std::collections::HashMap::new();
    let mut kept_count: std::collections::HashMap<_, usize> = std::collections::HashMap::new();
    let mut converged = Vec::with_capacity(entries.len());
    let mut changed = lifecycle_changed;

    for (idx, entry) in entries.iter().enumerate() {
        let Some(key) = convergence_identity(entry) else {
            // Non-prompt entry (fence / preset / dispatch / freeform): position-
            // and content-stable, never a convergence target.
            converged.push(entry.clone());
            continue;
        };

        let (identity, shape, lifecycle) = (&key.0, key.1, key.2);

        // The first sighting of an identity authors/mirrors it; the genesis
        // event for a struck copy is the struck terminal so a re-emit cannot
        // un-retire it (mirrors the CRDT `Struck` lattice join).
        let genesis_event = match lifecycle {
            QueueItemLifecycle::Live => {
                if identity.is_id_backed() {
                    QueueItemEvent::BacklogMirrored
                } else {
                    QueueItemEvent::OperatorAuthored
                }
            }
            QueueItemLifecycle::Struck => QueueItemEvent::StruckThrough,
        };

        // Lawful multiplicity for this key (how many copies survive):
        // - A pin-variant `do [#id]` group collapses to exactly one survivor
        //   regardless of snapshot count (`#qdedupsync` / `#pushpinaccum`).
        // - Otherwise a `do [#id]` **directive** identity is unbounded: identical
        //   directive twins are intentional "run it twice"
        //   (`#queue-dedup-destroys-intentional-duplicates`).
        // - Bare references and free-text lines collapse to the snapshot-authored
        //   multiplicity (min 1) — `#qdup-bare-id` / `#qauthorder` / `#rt83qflood`.
        // A bare `[#id]` reference subsumed by a live directive of the same id is
        // never kept — its lawful multiplicity is zero (`#brtc` cross-shape
        // subsumption above).
        let subsumed_by_directive = matches!(
            (shape, identity, lifecycle),
            (
                HeadShape::BareReference,
                agent_doc_element_queue::QueueItemIdentity::Id(id),
                QueueItemLifecycle::Live,
            ) if directive_ids.contains(id)
        );
        let pin_collapsing = matches!(
            identity,
            agent_doc_element_queue::QueueItemIdentity::Id(id) if pin_collapse_ids.contains(id)
        );
        let allowed = if subsumed_by_directive {
            0
        } else if pin_collapsing {
            1
        } else if shape == HeadShape::Directive && lifecycle == QueueItemLifecycle::Live {
            usize::MAX // intentional run-twice: never drop identical directive twins
        } else {
            authored.get(&key).copied().unwrap_or(0).max(1)
        };
        let seen = kept_count.entry(key.clone()).or_insert(0);

        if *seen >= allowed {
            // Re-emit beyond the authored multiplicity: drive the existing
            // identity's machine with the re-emit event and observe that it is a
            // HOLD (no state advance) — the structural anti-duplication no-op.
            // The copy is dropped.
            if let Some(machine) = machines.get(&key) {
                let before = machine.state();
                machine.send(genesis_event);
                debug_assert_eq!(
                    machine.state(),
                    before,
                    "re-emit of an already-kept identity must be a no-op transition"
                );
            }
            changed = true;
            continue;
        }

        // First lawful sighting (within authored multiplicity): keep it. Seed the
        // machine at its genesis state so later re-emits see an advanced state.
        let initial = match lifecycle {
            QueueItemLifecycle::Struck => QueueItemState::Struck,
            QueueItemLifecycle::Live => {
                if identity.is_id_backed() {
                    QueueItemState::Mirrored
                } else {
                    QueueItemState::Authored
                }
            }
        };
        machines
            .entry(key.clone())
            .or_insert_with(|| QueueItemMachine::new(initial));
        *seen += 1;

        // Apply the pin-variant survivor rewrite if this is the earliest member
        // of a collapsing id group.
        match (survivor_text.get(&idx), entry) {
            (Some(text), QueueEntry::Prompt(p)) => {
                if *text != p.text {
                    changed = true;
                }
                converged.push(QueueEntry::Prompt(QueuePrompt {
                    text: text.clone(),
                    multiline: false,
                }));
            }
            _ => converged.push(entry.clone()),
        }
    }

    if changed { Some(converged) } else { None }
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
            let mut completed = prompt.clone();
            completed.text = strip_in_progress_marker(&completed.text);
            result.push(QueueEntry::Completed(completed));
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
            let snap_head_text = strip_priority_markers(&s.text);
            let file_head_text = strip_priority_markers(&f.text);
            if snap_head_text == file_head_text {
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
            let snap_head_still_present = file_entries.iter().any(|entry| {
                matches!(
                    entry,
                    QueueEntry::Prompt(p) if strip_priority_markers(&p.text) == snap_head_text
                )
            });
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
    fn in_progress_marker_is_cosmetic_for_queue_identity() {
        assert_eq!(
            strip_priority_markers("🚧 :pushpin: do [#alpha]"),
            "do [#alpha]"
        );
        assert_eq!(
            strip_in_progress_marker("🚧 :pushpin: do [#alpha]"),
            ":pushpin: do [#alpha]"
        );
    }

    #[test]
    fn set_first_prompt_in_progress_moves_visible_marker() {
        let entries = parse("- 🚧 do [#old]\n- do [#new]\n").unwrap();
        let marked = set_first_prompt_in_progress(&entries, true).unwrap();
        assert_eq!(render(&marked), "- 🚧 do [#old]\n- do [#new]\n");

        let entries = parse("- ~~🚧 do [#old]~~\n- do [#new]\n").unwrap();
        let marked = set_first_prompt_in_progress(&entries, true).unwrap();
        assert_eq!(render(&marked), "- ~~do [#old]~~\n- 🚧 do [#new]\n");

        let entries = parse("- 🚧 /clear\n- do [#new]\n").unwrap();
        let marked = set_first_prompt_in_progress(&entries, true).unwrap();
        assert_eq!(render(&marked), "/clear\n- do [#new]\n");
    }

    #[test]
    fn set_prompts_in_progress_projects_multiple_active_heads() {
        let entries = parse("- 🚧 do [#old]\n- do [#alpha]\n- do [#beta]\n").unwrap();
        let targets = vec!["do [#alpha]".to_string(), "do [#beta]".to_string()];
        let marked = set_prompts_in_progress(&entries, &targets).unwrap();

        assert_eq!(
            render(&marked),
            "- do [#old]\n- 🚧 do [#alpha]\n- 🚧 do [#beta]\n"
        );
    }

    #[test]
    fn in_progress_marker_detection_allows_priority_prefix() {
        assert!(has_in_progress_marker(":round_pushpin: 🚧 do [#alpha]"));
        assert_eq!(
            strip_in_progress_marker(":round_pushpin: 🚧 do [#alpha]"),
            ":round_pushpin: do [#alpha]"
        );

        let entries = parse("- :round_pushpin: 🚧 do [#alpha]\n").unwrap();
        let marked = set_prompts_in_progress(&entries, &[]).unwrap();
        assert_eq!(render(&marked), "- :round_pushpin: do [#alpha]\n");
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
    fn operator_authored_prompt_identities_detects_operator_added_unpinned_line() {
        // #7r2s/#qauthorderpin: the operator typed a brand-new `do [#manual]`
        // line with no pin. It is absent from the snapshot and was NOT appended
        // by backlog sync, so the priority sort should receive a stable identity
        // instead of mutating the visible text with `:pushpin:`.
        let snapshot = parse("- do [#a]\n").unwrap();
        let current = parse("- do [#manual]\n- do [#a]\n").unwrap();
        let synced: std::collections::HashSet<String> = std::collections::HashSet::new();

        let identities = operator_authored_prompt_identities(&snapshot, &current, &synced);
        let expected: std::collections::HashSet<String> =
            ["do [#manual]".to_string()].into_iter().collect();
        assert_eq!(identities, expected);
        assert!(
            annotate_manual_queue_additions(&snapshot, &current, &synced).is_none(),
            "manual additions are now anchored by identity, not visible pins"
        );
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
            operator_authored_prompt_identities(&snapshot, &current, &synced).is_empty(),
            "binary-synced and already-pinned new lines are not authored anchors"
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
    fn sync_mode_mirrors_backlog_order_and_preserves_manual_entries() {
        let entries = parse("- do [#old]\npreset spec\n").unwrap();
        let synced =
            sync_backlog_into_queue(&entries, &ids(&["a", "b"]), BacklogQueueSyncMode::Sync)
                .expect("queue should change");
        assert_eq!(render(&synced), "- do [#a]\n- do [#b]\npreset spec\n");
    }

    #[test]
    fn sync_mode_preserves_manual_free_text_queue_edits() {
        let entries = parse("- test\n- do [#old]\n").unwrap();
        let synced =
            sync_backlog_into_queue(&entries, &ids(&["a", "b"]), BacklogQueueSyncMode::Sync)
                .expect("queue should change");
        assert_eq!(render(&synced), "- test\n- do [#a]\n- do [#b]\n");
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
    fn append_mode_treats_multi_id_line_as_all_present() {
        // #provauth2: a manually-authored multi-id line `[#a] [#b] [#c]` must
        // mark a, b AND c as already-present, so the backlog→queue mirror does
        // NOT re-add the trailing ids as duplicate `do [#id]` lines. Before the
        // multi-id fix, only `a` (the first `#id`) counted, so b and c
        // proliferated as duplicates on every preflight.
        let entries = parse("- [#a] [#b] [#c]\n").unwrap();
        assert!(
            sync_backlog_into_queue(
                &entries,
                &ids(&["a", "b", "c"]),
                BacklogQueueSyncMode::Append
            )
            .is_none(),
            "every id on a multi-id line must count as present (no duplicate re-add)"
        );
    }

    #[test]
    fn append_mode_adds_only_the_genuinely_missing_id_past_a_multi_id_line() {
        // The multi-id line covers a/b/c; only `d` is genuinely absent and is
        // the single item appended.
        let entries = parse("- [#a] [#b] [#c]\n").unwrap();
        let synced = sync_backlog_into_queue(
            &entries,
            &ids(&["a", "b", "c", "d"]),
            BacklogQueueSyncMode::Append,
        )
        .expect("a genuinely missing id should append");
        assert_eq!(render(&synced), "- [#a] [#b] [#c]\n- do [#d]\n");
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
        // #qauthorder: the pre-existing free-text line is now position-locked at
        // its authored slot (anchored without a pin), so even WITHOUT
        // append-stability (empty backlog set) the `do [#a]` item never floats
        // above it — the pre-#qauthorder "prepend by rank" escape hatch is gone.
        // Free-text anchoring subsumes append-stability for the free-text-vs-do
        // case; append-stability still governs do-vs-do ordering.
        assert!(
            sort_prompts_by_priority(&entries, &rank, &std::collections::HashSet::new()).is_none(),
            "free-text is position-locked: a backlog item must not bubble above it"
        );
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
            "- [#sqedit-race]\n",                  // bare duplicate → collapse
            "- :pushpin: [#qpausemix-verify]\n",   // pinned bare dup → collapse
            "- #sqedit-race\n",                    // unbracketed bare dup → collapse
            "- do [#sqedit-race]\n",               // `do` directive → PRESERVED
            "- do [#sqedit-race]\n",               // intentional `do` duplicate → PRESERVED
            "- #sqedit-race continue the drain\n", // free-text citing an id → PRESERVED
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

    // ---- #qauthorder: operator-authored free-text order ----

    #[test]
    fn dedup_free_text_heads_collapses_duplicate_operator_line() {
        // The live repro: an operator's free-text line (committed once) re-emitted
        // twice by convergence. Free-text dedup collapses the artifact copy back
        // to the authored count, leaving id-backed heads untouched.
        let snapshot =
            parse("- do [#a]\n- keep operator order, do not bubble to the top\n- do [#b]\n")
                .unwrap();
        let entries = parse(concat!(
            "- do [#a]\n",
            "- keep operator order, do not bubble to the top\n",
            "- keep operator order, do not bubble to the top\n",
            "- do [#b]\n",
        ))
        .unwrap();
        let deduped = dedup_free_text_heads(&entries, &snapshot)
            .expect("artifact copy beyond authored count should collapse");
        assert_eq!(
            render(&deduped),
            "- do [#a]\n- keep operator order, do not bubble to the top\n- do [#b]\n"
        );
    }

    #[test]
    fn dedup_free_text_heads_collapses_pin_variant_and_struck_duplicates() {
        // A re-emit that only differs by an injected `:pushpin:` still collapses
        // (the snapshot committed one struck copy; convergence doubled it).
        let snapshot = parse("- ~~items added by operator stay in document order~~\n").unwrap();
        let entries = parse(concat!(
            "- ~~:pushpin: items added by operator stay in document order~~\n",
            "- ~~items added by operator stay in document order~~\n",
        ))
        .unwrap();
        let deduped = dedup_free_text_heads(&entries, &snapshot)
            .expect("struck pin-variant artifact copy should collapse");
        assert_eq!(
            render(&deduped),
            "- ~~:pushpin: items added by operator stay in document order~~\n"
        );
    }

    #[test]
    fn dedup_free_text_heads_preserves_intentional_authored_duplicate() {
        // #queue-dedup-destroys-intentional-duplicates: two identical free-text
        // lines that the operator COMMITTED (present twice in the snapshot) are
        // intentional "run it twice" intent and must survive.
        let snapshot = parse("- do deploy\n- do deploy\n").unwrap();
        let entries = parse("- do deploy\n- do deploy\n").unwrap();
        assert!(
            dedup_free_text_heads(&entries, &snapshot).is_none(),
            "authored duplicate count must be preserved"
        );
    }

    #[test]
    fn dedup_free_text_heads_preserves_distinct_lines_and_id_heads() {
        // Distinct free-text lines stay; intentional `do [#id]` duplicates are the
        // id-keyed dedups' concern, not this one (left untouched here).
        let entries = parse("- first note\n- second note\n- do [#a]\n- do [#a]\n").unwrap();
        assert!(
            dedup_free_text_heads(&entries, &entries).is_none(),
            "distinct free-text + do-dups → no free-text collapse"
        );
    }

    #[test]
    fn dedup_free_text_heads_keeps_live_and_struck_pair() {
        // A live + struck line of the same text is a legitimate "in progress +
        // done" pair (different variant key), not a convergence duplicate.
        let entries = parse("- review the draft\n- ~~review the draft~~\n").unwrap();
        assert!(
            dedup_free_text_heads(&entries, &[]).is_none(),
            "live + struck same-text pair must be preserved"
        );
    }

    #[test]
    fn dedup_free_text_heads_collapses_multiline_phantom_pin_flood() {
        // #rt83qflood: a multiline `:round_pushpin:` actor-switch paste block (a
        // pinned operator line plus a fenced route-error body) re-emits verbatim
        // under stale-CRDT / supervisor convergence, flooding the queue with
        // near-identical copies. Before the multiline extension `free_text_dedup_key`
        // returned `None` for multiline prompts, so these were never deduped. The
        // operator committed ONE; the phantom copies — including a `:pushpin:` pin
        // variant and a whitespace-only variant — collapse back to the authored
        // multiplicity, just like the single-line free-text case.
        let pin = concat!(
            "---\n",
            ":round_pushpin: switch harness on sampleportal\n",
            "route defer error\n",
            "---\n",
        );
        let snapshot = parse(pin).unwrap();
        let entries = parse(concat!(
            // authored copy
            "---\n:round_pushpin: switch harness on sampleportal\nroute defer error\n---\n",
            // phantom: pin-variant (:pushpin:) — strip_priority_markers collapses it
            "---\n:pushpin: switch harness on sampleportal\nroute defer error\n---\n",
            // phantom: whitespace-only variant — normalize_multiline_dedup_text collapses it
            "---\n:round_pushpin:   switch harness on sampleportal\n\nroute defer error\n---\n",
        ))
        .unwrap();
        let deduped = dedup_free_text_heads(&entries, &snapshot)
            .expect("multiline phantom-pin copies beyond authored count must collapse");
        let multiline_pins = deduped
            .iter()
            .filter(|e| matches!(e, QueueEntry::Prompt(p) if p.multiline))
            .count();
        assert_eq!(
            multiline_pins, 1,
            "3 phantom multiline pins (pin + whitespace variants) collapse to the 1 authored copy: {deduped:?}"
        );
    }

    #[test]
    fn dedup_free_text_heads_preserves_distinct_multiline_pins() {
        // Two genuinely-distinct multiline pins (different bodies) are not a
        // convergence artifact and must both survive — the snapshot-authored
        // multiplicity guard keys them separately.
        let entries = parse(concat!(
            "---\n:round_pushpin: switch harness on sampleportal\nroute defer error\n---\n",
            "---\n:round_pushpin: restart supervisor on sampleorders\ndifferent body\n---\n",
        ))
        .unwrap();
        assert!(
            dedup_free_text_heads(&entries, &entries).is_none(),
            "distinct multiline pins must both be preserved"
        );
    }

    #[test]
    fn operator_authored_identity_set_does_not_pin_new_prompt_text() {
        // #qauthorder/#qauthorderpin: brand-new operator free-text and id-backed
        // lines are NOT auto-pinned; they are position-locked by stable identity
        // in the sort instead.
        let snapshot = parse("- do [#a]\n").unwrap();
        let current = parse("- do [#a]\n- hold this slot, do not bubble\n- do [#b]\n").unwrap();
        let synced: std::collections::HashSet<String> = std::collections::HashSet::new();
        let identities = operator_authored_prompt_identities(&snapshot, &current, &synced);
        let expected: std::collections::HashSet<String> = [
            "hold this slot, do not bubble".to_string(),
            "do [#b]".to_string(),
        ]
        .into_iter()
        .collect();
        assert_eq!(identities, expected);
        assert_eq!(
            render(&current),
            "- do [#a]\n- hold this slot, do not bubble\n- do [#b]\n",
            "operator-authored text stays marker-free"
        );
    }

    #[test]
    fn operator_authored_id_backed_line_is_position_locked_in_priority_sort() {
        // #qauthorderpin: the operator manually inserted an id-backed queue head
        // in the middle. Even if its backlog rank would make it bubble to the
        // front, the identity-aware priority sort holds that authored slot and
        // reorders only the other prompts around it.
        let entries = parse("- do [#a]\n- do [#manual]\n- do [#b]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("manual".to_string(), 1u8);
        rank.insert("b".to_string(), 2u8);
        rank.insert("a".to_string(), 9u8);
        let backlog: std::collections::HashSet<String> =
            ["manual".to_string(), "a".to_string(), "b".to_string()]
                .into_iter()
                .collect();
        let authored: std::collections::HashSet<String> =
            ["do [#manual]".to_string()].into_iter().collect();
        let sorted =
            sort_prompts_by_priority_with_operator_authored(&entries, &rank, &backlog, &authored)
                .expect("non-authored prompts reorder around the manual slot");
        assert_eq!(
            render(&sorted),
            "- do [#b]\n- do [#manual]\n- do [#a]\n",
            "manual id-backed prompt keeps slot 1 without a visible pin"
        );
    }

    #[test]
    fn operator_authored_id_backed_line_is_position_locked_in_dag_path() {
        // Same anchoring on the auto-DAG path: dependency edges can reorder the
        // movable prompts around the operator-authored id line without injecting
        // `:pushpin:`.
        let entries = parse("- do [#a]\n- do [#manual]\n- do [#b]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8);
        rank.insert("manual".to_string(), 2u8);
        rank.insert("b".to_string(), 3u8);
        let mut deps: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        deps.insert("a".to_string(), vec!["b".to_string()]); // #a after #b
        let backlog: std::collections::HashSet<String> =
            ["manual".to_string(), "a".to_string(), "b".to_string()]
                .into_iter()
                .collect();
        let authored: std::collections::HashSet<String> =
            ["do [#manual]".to_string()].into_iter().collect();
        let sorted =
            sort_prompts_by_dag_with_operator_authored(&entries, &rank, &deps, &backlog, &authored)
                .expect("dependency edge reorders movable prompts around the manual slot");
        assert_eq!(
            render(&sorted),
            "- do [#b]\n- do [#manual]\n- do [#a]\n",
            "manual id-backed prompt keeps slot 1 in DAG ordering"
        );
    }

    #[test]
    fn free_text_operator_line_is_position_locked_in_priority_sort() {
        // #qauthorder: a free-text line authored between two do-prompts stays at
        // its slot while the do-prompts reorder around it by rank — it neither
        // bubbles to the top nor sinks.
        let entries = parse("- do [#a]\n- operator note in the middle\n- do [#b]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 5u8);
        rank.insert("b".to_string(), 1u8); // better rank → wants to be first
        let sorted = sort_prompts_by_priority(&entries, &rank, &std::collections::HashSet::new())
            .expect("do-prompts reorder around the anchored free-text line");
        assert_eq!(
            render(&sorted),
            "- do [#b]\n- operator note in the middle\n- do [#a]\n",
            "free-text anchored at slot 1; do-prompts fill the rest by rank"
        );
    }

    #[test]
    fn free_text_operator_line_position_locked_in_dag_path() {
        // The same anchoring holds on the auto-dag path: a free-text line stays
        // put while `after=` edges reorder the do-prompts around it.
        let entries = parse("- do [#a]\n- operator note\n- do [#b]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8);
        rank.insert("b".to_string(), 5u8);
        let mut deps: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        deps.insert("a".to_string(), vec!["b".to_string()]); // #a after #b
        let sorted = sort_prompts_by_dag(&entries, &rank, &deps, &std::collections::HashSet::new())
            .expect("dep edge reorders the do-prompts around the anchor");
        assert_eq!(
            render(&sorted),
            "- do [#b]\n- operator note\n- do [#a]\n",
            "free-text anchored at slot 1; #b precedes #a by edge"
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
    fn dedup_pin_variant_do_heads_collapses_pin_plus_bare_twin() {
        // #qdedupsync live repro: the queue held `:pushpin: do [#6b5hwire]` AND a
        // bare `do [#6b5hwire]` — same id, differing only by the pin marker (a
        // pin-accumulation artifact). They collapse to a single instance kept at
        // the earliest position, carrying the strongest (operator) pin.
        let entries = parse(concat!(
            "- do [#6b5h]\n",
            "- do [#6b5hwire]\n",           // bare twin, earliest for this id
            "- :pushpin: do [#6b5hwire]\n", // pinned twin → collapse, strongest pin wins
            "- do [#tb4q]\n",
        ))
        .unwrap();
        let deduped = dedup_pin_variant_do_heads(&entries)
            .expect("pin-variant `do [#id]` duplicates should collapse");
        assert_eq!(
            render(&deduped),
            concat!(
                "- do [#6b5h]\n",
                "- :pushpin: do [#6b5hwire]\n", // kept at earliest position, strongest pin
                "- do [#tb4q]\n",
            ),
            "pin+bare twin collapses to one strongest-pin head at the earliest slot: {deduped:?}"
        );
    }

    #[test]
    fn dedup_pin_variant_do_heads_preserves_identical_run_twice() {
        // Two textually IDENTICAL `do [#id]` heads are intentional "run it twice"
        // intent (#queue-dedup-destroys-intentional-duplicates) — they must survive.
        let entries = parse("- do [#a]\n- do [#a]\n- :pushpin: do [#b]\n").unwrap();
        assert!(
            dedup_pin_variant_do_heads(&entries).is_none(),
            "identical do-dups (no pin disagreement) must be preserved as run-twice intent"
        );
    }

    #[test]
    fn dedup_pin_variant_do_heads_prefers_operator_over_agent_pin() {
        // When the same id appears with an agent pin AND an operator pin, the
        // operator pin (`:pushpin:`) is the strongest and survives.
        let entries = parse(concat!(
            "- :round_pushpin: do [#x]\n", // agent pin, earliest
            "- :pushpin: do [#x]\n",       // operator pin → strongest
        ))
        .unwrap();
        let deduped = dedup_pin_variant_do_heads(&entries)
            .expect("agent+operator pin variants of one id should collapse");
        assert_eq!(
            render(&deduped),
            "- :pushpin: do [#x]\n",
            "operator pin is strongest and is kept at the earliest slot: {deduped:?}"
        );
    }

    #[test]
    fn dedup_pin_variant_do_heads_ignores_free_text_and_distinct_ids() {
        // Free-text heads and distinct id heads are never grouped/collapsed.
        let entries =
            parse("- do [#a]\n- do [#b]\n- a free text question?\n- #c continue the drain\n")
                .unwrap();
        assert!(
            dedup_pin_variant_do_heads(&entries).is_none(),
            "distinct ids + free-text must not collapse"
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
    fn parse_completed_annotated_free_text_item() {
        // `#qstrikenote`: the visible auto-struck explanation lives outside the
        // `~~...~~` wrapper, but the queue runtime must still treat the row as
        // completed. Otherwise maintenance/journal replay sees it as a live
        // prompt and can re-strike it into `~~~~text~~ note~~`.
        let body = "- ~~JB `File Cache Conflict` occurrences~~ — auto-struck: answered this cycle (#ftstrike)\n";
        let entries = parse(body).unwrap();

        assert_eq!(entries.len(), 1);
        assert!(
            matches!(&entries[0], QueueEntry::Completed(p) if p.text == "JB `File Cache Conflict` occurrences"),
            "{entries:?}"
        );
        assert_eq!(prompts(&entries).len(), 0);
    }

    #[test]
    fn parse_completed_nested_annotated_restrike_artifact() {
        let body = "- ~~~~JB `File Cache Conflict` occurrences~~ — auto-struck: answered this cycle (#ftstrike)~~\n";
        let entries = parse(body).unwrap();

        assert!(
            matches!(&entries[0], QueueEntry::Completed(p) if p.text == "JB `File Cache Conflict` occurrences"),
            "{entries:?}"
        );
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
    fn parse_separator_terminated_freetext_as_multiline_prompt() {
        let body = concat!(
            "JB `Run Agent Doc` on agent-loop.md after switching from claude to codex.\n",
            "The actor record did not switch.\n",
            "```\n",
            "Error: authoritative actor record is bound to harness claude-code, not codex\n",
            "```\n",
            "---\n",
            "- do [#next]\n",
        );
        let entries = parse(body).unwrap();
        let prompts = prompts(&entries);
        assert_eq!(prompts.len(), 2);
        assert!(prompts[0].multiline);
        assert!(prompts[0].text.contains("actor record did not switch"));
        assert_eq!(prompts[1].text, "do [#next]");
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

    /// #sqedit-race Phase 3: prove the queue parse/render round-trip is a fixed
    /// point on an already-malformed queue — it must **converge, not amplify**.
    ///
    /// The live-corruption shapes a prior supervisor/editor race left behind
    /// (double-pinned heads, stray `~~~done` openers without closers, runs of
    /// empty `---` separators that previously split or dropped a real head like
    /// `#fbwire`) must collapse to a stable canonical form after a single
    /// `render(parse(x))` pass, and every subsequent pass must be a no-op. If the
    /// round-trip ever grew the body (re-mangled an entry, multiplied a pin, or
    /// re-injected a stray separator) it would feed the qchurn loop forever.
    #[test]
    fn render_parse_is_fixed_point_on_malformed_queue() {
        let normalize =
            |s: &str| render(&parse(s).expect("parse must never fail on a polluted queue"));
        let pin_count = |s: &str| s.matches(":pushpin:").count();

        // The compounded malformed shape: empty `---` separator runs top and
        // bottom, a double-pinned id head, an orphaned `~~~done` opener with no
        // matching closer, a stray `:round_pushpin:` note line, and two real
        // directive heads plus a free-text question that must all survive.
        let malformed = "\
---
---
- :pushpin: :pushpin: do [#dup]
~~~done
- do [#real]
:round_pushpin: orphan note line
---
---
- a genuine free-text question?
";

        let once = normalize(malformed);
        let twice = normalize(&once);

        // (1) The round-trip is a fixed point: a second pass changes nothing.
        assert_eq!(
            once, twice,
            "render(parse(x)) must be a fixed point — got amplification:\n--- once ---\n{once}\n--- twice ---\n{twice}"
        );

        // (2) No real directive/free-text head is dropped (the #fbwire-style
        //     head-loss amplification bug).
        let parsed_once = parse(&once).unwrap();
        let prompts_once = prompts(&parsed_once);
        let texts: Vec<&str> = prompts_once.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(
            prompts_once.len(),
            3,
            "all three real heads must survive normalization, got {texts:?}"
        );
        assert!(texts.iter().any(|t| t.contains("do [#dup]")));
        assert!(texts.iter().any(|t| t.contains("do [#real]")));
        assert!(
            texts
                .iter()
                .any(|t| t.contains("a genuine free-text question?"))
        );

        // (3) Pins converge, never amplify: the double pin is preserved verbatim
        //     (canonical-collapse to a single pin is `apply_operator_pin`'s job
        //     during maintenance), but the count must not grow pass-over-pass.
        assert_eq!(
            pin_count(&once),
            pin_count(&twice),
            "pin markers must not amplify across round-trips"
        );

        // (4) The stray empty `---` separator runs are collapsed away, not
        //     re-injected or re-split into more separators.
        assert!(
            !once.contains("---"),
            "empty `---` separator runs must collapse, not survive/amplify:\n{once}"
        );
    }

    /// #sqedit-race Phase 3 (property form): the round-trip is a fixed point over
    /// a family of independently-malformed queue bodies, so the convergence
    /// guarantee is not specific to one hand-picked corruption shape.
    #[test]
    fn render_parse_fixed_point_property_over_malformed_family() {
        let normalize = |s: &str| render(&parse(s).expect("parse must never fail"));
        let cases = [
            // double pins + trailing empty fence run
            "- :pushpin: :pushpin: do [#a]\n---\n---\n",
            // orphan completed-fence opener wedged above a real item
            "~~~done\n- do [#b]\n",
            // interleaved stray separators and a free-text head
            "---\n- first?\n---\n---\n- second?\n",
            // stray prose note between two id heads
            "- do [#c]\nrandom pasted log line\n- do [#d]\n",
            // a real multiline `---` fenced head sandwiched in separator runs
            "---\n---\nmulti line\nhead body\n---\n- do [#e]\n",
            // #qdedupsync: a pinned + bare same-id `do [#id]` family. render(parse)
            // itself must be a fixed point (it preserves both heads verbatim — the
            // collapse is a separate maintenance step, `dedup_pin_variant_do_heads`),
            // so the round-trip must neither drop nor multiply either variant.
            "- :pushpin: do [#6b5hwire]\n- do [#6b5hwire]\n",
            // #qauthorder: a duplicated free-text operator line (one pin-injected,
            // one struck). render(parse) preserves all variants verbatim — the
            // collapse is a separate maintenance step (`dedup_free_text_heads`) —
            // so the round-trip must neither drop nor multiply any line.
            "- :pushpin: keep operator order\n- keep operator order\n- ~~keep operator order~~\n",
        ];
        for case in cases {
            let once = normalize(case);
            let twice = normalize(&once);
            assert_eq!(
                once, twice,
                "render(parse(x)) must be a fixed point for case:\n{case}\n--- once ---\n{once}\n--- twice ---\n{twice}"
            );
        }
    }

    // ---- Unified SM-driven convergence (`#cgfx` / `#queuestatemachine2`) ----

    fn p(text: &str) -> QueueEntry {
        QueueEntry::Prompt(QueuePrompt {
            text: text.to_string(),
            multiline: false,
        })
    }
    fn c(text: &str) -> QueueEntry {
        QueueEntry::Completed(QueuePrompt {
            text: text.to_string(),
            multiline: false,
        })
    }

    /// Apply convergence, returning the converged entries (or the input unchanged
    /// when the queue is already at its lawful fixpoint).
    fn converge(entries: &[QueueEntry], snapshot: &[QueueEntry]) -> Vec<QueueEntry> {
        converge_queue_via_lifecycle(entries, snapshot).unwrap_or_else(|| entries.to_vec())
    }

    /// `#qdedupsync` / `#pushpinaccum`: a pinned `do [#id]` accumulated alongside
    /// its bare twin collapses to one strongest-pin survivor at the earliest slot.
    #[test]
    fn converge_collapses_pin_variant_do_head() {
        let entries = vec![p(":pushpin: do [#6b5hwire]"), p("do [#6b5hwire]")];
        let out = converge(&entries, &[]);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out, vec![p(":pushpin: do [#6b5hwire]")]);
    }

    /// `#queue-dedup-destroys-intentional-duplicates`: textually-identical
    /// `do [#id]` directive twins are intentional "run it twice" and survive.
    #[test]
    fn converge_preserves_identical_directive_twins() {
        let entries = vec![p("do [#deploy]"), p("do [#deploy]")];
        assert!(
            converge_queue_via_lifecycle(&entries, &[]).is_none(),
            "identical directive twins must be left untouched"
        );
    }

    /// `#qdup-bare-id` + `#brtc` cross-shape subsumption: bare `[#id]` references
    /// re-emitted by the mirror/CRDT replay collapse to zero when a live `do [#id]`
    /// **directive** of the same id is present — the actionable directive subsumes
    /// the pure reference, so an identity converges to exactly one visible head.
    #[test]
    fn converge_bare_reference_subsumed_by_directive() {
        let entries = vec![
            p("[#sqedit-race]"),
            p("[#sqedit-race]"),
            p("do [#sqedit-race]"),
        ];
        let out = converge(&entries, &[]);
        assert_eq!(out, vec![p("do [#sqedit-race]")], "{out:?}");
    }

    /// Without a directive, bare `[#id]` reference dups still collapse to one
    /// (the original `#qdup-bare-id` behavior).
    #[test]
    fn converge_collapses_bare_reference_dups_without_directive() {
        let entries = vec![p("[#sqedit-race]"), p("[#sqedit-race]")];
        let out = converge(&entries, &[]);
        assert_eq!(out, vec![p("[#sqedit-race]")], "{out:?}");
    }

    /// `#qauthorder`: a free-text operator line re-emitted beyond its snapshot
    /// multiplicity collapses to the authored count; position is preserved.
    #[test]
    fn converge_collapses_free_text_to_snapshot_multiplicity() {
        let snapshot = vec![p("queue items are not bubbled to the top")];
        let entries = vec![
            p("queue items are not bubbled to the top"),
            p("queue items are not bubbled to the top"),
        ];
        let out = converge(&entries, &snapshot);
        assert_eq!(
            out,
            vec![p("queue items are not bubbled to the top")],
            "{out:?}"
        );
    }

    /// A genuinely twice-authored free-text line keeps both copies.
    #[test]
    fn converge_preserves_double_authored_free_text() {
        let snapshot = vec![p("run the smoke test"), p("run the smoke test")];
        let entries = snapshot.clone();
        assert!(
            converge_queue_via_lifecycle(&entries, &snapshot).is_none(),
            "double-authored free-text must survive at its authored multiplicity"
        );
    }

    /// Live vs struck of the same identity are distinct lawful states — a genuine
    /// "in progress" + "done" pair survives.
    #[test]
    fn converge_keeps_live_and_struck_pair() {
        let entries = vec![p("ship the release"), c("ship the release")];
        assert!(
            converge_queue_via_lifecycle(&entries, &[]).is_none(),
            "a live + struck pair of the same text is not a duplicate"
        );
    }

    /// `#qeditdupguard`: snapshot-terminal queue lifecycle evidence is
    /// monotonic. A stale editor/live-buffer flush that replays an unstruck copy
    /// of a head the snapshot already struck must converge back to struck instead
    /// of resurrecting runnable work.
    #[test]
    fn converge_snapshot_struck_dominates_stale_live_reemit() {
        let snapshot = vec![c(":pushpin: do [#qeditdup] [#qftloss#qftloss]")];
        let entries = vec![p(":pushpin: do [#qeditdup] [#qftloss#qftloss]")];

        let out = converge(&entries, &snapshot);

        assert_eq!(
            out,
            vec![c(":pushpin: do [#qeditdup] [#qftloss#qftloss]")],
            "a stale live re-emit must render as the snapshot's struck representative"
        );
    }

    /// When the current queue already carries the struck representative, an
    /// adjacent stale live copy is just replay residue and should disappear.
    #[test]
    fn converge_current_struck_drops_stale_live_reemit() {
        let snapshot = vec![c("do [#qeditdup] [#qftloss#qftloss]")];
        let entries = vec![
            p("do [#qeditdup] [#qftloss#qftloss]"),
            c("do [#qeditdup] [#qftloss#qftloss]"),
        ];

        let out = converge(&entries, &snapshot);

        assert_eq!(
            out,
            vec![c("do [#qeditdup] [#qftloss#qftloss]")],
            "current struck evidence should absorb the stale live duplicate"
        );
    }

    /// `#queue-operator-pin-position-lock` / `#qauthorder`: convergence never
    /// reorders. An operator free-text line authored between two id heads holds
    /// its slot across a re-emit storm of the surrounding heads.
    #[test]
    fn converge_preserves_operator_authored_position() {
        let snapshot = vec![p("do [#first]"), p("keep me here"), p("do [#second]")];
        // A re-emit storm duplicates the id heads around the operator line.
        let entries = vec![
            p("do [#first]"),
            p("[#first]"),
            p("keep me here"),
            p("keep me here"),
            p("do [#second]"),
            p("[#second]"),
        ];
        let out = converge(&entries, &snapshot);
        // Bare references collapse, free-text collapses to authored 1, directive
        // heads survive — and the operator line stays between the two id heads.
        let texts: Vec<&str> = out
            .iter()
            .filter_map(|e| match e {
                QueueEntry::Prompt(pr) => Some(pr.text.as_str()),
                _ => None,
            })
            .collect();
        let first = texts.iter().position(|t| *t == "do [#first]").unwrap();
        let keep = texts.iter().position(|t| *t == "keep me here").unwrap();
        let second = texts.iter().position(|t| *t == "do [#second]").unwrap();
        assert!(
            first < keep && keep < second,
            "operator line must keep its authored slot: {texts:?}"
        );
        assert_eq!(
            texts.iter().filter(|t| **t == "keep me here").count(),
            1,
            "free-text re-emit must collapse: {texts:?}"
        );
    }

    /// Idempotence: converging the output again is a no-op (`None`). This is the
    /// fixpoint guarantee — a re-emit storm settles to a stable queue.
    #[test]
    fn converge_is_idempotent_over_known_duplication_shapes() {
        let cases: Vec<(Vec<QueueEntry>, Vec<QueueEntry>)> = vec![
            // #qdedupsync / #pushpinaccum — pin-variant accumulation
            (vec![p(":pushpin: do [#a]"), p("do [#a]")], vec![]),
            // #qdup-bare-id — bare reference mirror dups
            (vec![p("[#b]"), p("[#b]"), p("[#b]")], vec![]),
            // #qauthorder — free-text re-emit beyond snapshot
            (
                vec![p("operator note"), p("operator note")],
                vec![p("operator note")],
            ),
            // #rt83qflood — multiline phantom-pin flood (keyed via from_prompt)
            (
                vec![
                    QueueEntry::Prompt(QueuePrompt {
                        text: ":round_pushpin: switch actor\nroute error body".into(),
                        multiline: true,
                    }),
                    QueueEntry::Prompt(QueuePrompt {
                        text: ":round_pushpin: switch actor\nroute error body".into(),
                        multiline: true,
                    }),
                ],
                vec![],
            ),
            // mixed shapes + intentional directive twin survives
            (
                vec![p("do [#c]"), p("do [#c]"), p("[#c]"), p("[#c]"), p("free")],
                vec![],
            ),
        ];
        for (entries, snapshot) in cases {
            let once = converge(&entries, &snapshot);
            assert!(
                converge_queue_via_lifecycle(&once, &snapshot).is_none(),
                "convergence must be a fixpoint:\n{entries:?}\n--- once ---\n{once:?}"
            );
        }
    }

    proptest::proptest! {
        /// **The `#cgfx` idempotence / no-op-re-emit property.** For an arbitrary
        /// queue assembled from the duplication shapes that `#rt83qflood`,
        /// `#qdedupsync`, `#qauthorder`, `#qdup-bare-id`, and `#pushpinaccum` each
        /// patched — id-backed directive heads (pinned + bare variants), bare
        /// `[#id]` reference heads, and free-text operator lines, each re-emitted
        /// an arbitrary number of extra times across the queue — convergence is a
        /// **fixpoint**: re-running it on its own output is a guaranteed no-op.
        /// Re-injecting an identity that already has a lawful representative is a
        /// no-op transition, so a re-emit storm settles to a stable queue with no
        /// new duplicates ever introduced.
        #[test]
        fn converge_via_lifecycle_is_idempotent_under_reemit_storms(
            // Each shape token re-emits one of the historical duplication shapes;
            // 0..=5 ids, each with a random multiset of variants.
            id_kinds in proptest::collection::vec(0u8..6, 0..12),
            free_text_reemits in 0usize..6,
        ) {
            use proptest::prelude::*;
            // Deterministically build a queue from the shape tokens. The same id
            // surfaces under several shapes (pinned directive / bare directive /
            // bare reference) so pin-variant + bare-reference collapse both fire.
            let mut entries: Vec<QueueEntry> = Vec::new();
            for (i, kind) in id_kinds.iter().enumerate() {
                let id = format!("id{}", i % 4); // reuse ids → cross-shape collisions
                match kind {
                    0 => entries.push(p(&format!("do [#{id}]"))),
                    1 => entries.push(p(&format!(":pushpin: do [#{id}]"))),
                    2 => entries.push(p(&format!("_prioritized_ do [#{id}]"))),
                    3 => entries.push(p(&format!("[#{id}]"))),
                    4 => entries.push(p(&format!("#{id}"))),
                    _ => entries.push(c(&format!("do [#{id}]"))),
                }
            }
            for _ in 0..free_text_reemits {
                entries.push(p("operator authored prose line"));
            }

            // No snapshot → every free-text/bare identity collapses to one; only
            // intentional identical directive twins survive.
            let once = converge_queue_via_lifecycle(&entries, &[]);
            let converged = once.clone().unwrap_or_else(|| entries.clone());

            // FIXPOINT: a second convergence over the output is a no-op.
            prop_assert!(
                converge_queue_via_lifecycle(&converged, &[]).is_none(),
                "convergence is not a fixpoint:\n{entries:?}\n--- converged ---\n{converged:?}"
            );

            // NO-OP RE-EMIT: re-appending an already-present **artifact-shaped**
            // identity (bare `[#id]` reference or free-text line) to the converged
            // queue does not increase its converged head count — re-injection is
            // structurally absorbed. Identical `do [#id]` *directive* twins are
            // intentional "run it twice" (#queue-dedup-destroys-intentional-
            // duplicates), so a directive re-emit is excluded from this assertion;
            // its multiplication is the lawful authored behavior, not a dup bug.
            let storm_target = converged.iter().find_map(|e| match e {
                QueueEntry::Prompt(q) if head_shape(q) != HeadShape::Directive => Some(q.clone()),
                _ => None,
            });
            if let Some(first) = storm_target {
                let mut stormed = converged.clone();
                for _ in 0..3 {
                    stormed.push(QueueEntry::Prompt(first.clone()));
                }
                let restormed = converge_queue_via_lifecycle(&stormed, &converged)
                    .unwrap_or(stormed);
                let count_in = |v: &[QueueEntry], t: &str| {
                    v.iter()
                        .filter(|e| matches!(e, QueueEntry::Prompt(q) if q.text == t))
                        .count()
                };
                prop_assert!(
                    count_in(&restormed, &first.text) <= count_in(&converged, &first.text).max(1),
                    "re-emit storm of artifact-shaped {:?} must not multiply the identity\n--- converged ---\n{converged:?}\n--- restormed ---\n{restormed:?}",
                    first.text
                );
            }
        }
    }
}
