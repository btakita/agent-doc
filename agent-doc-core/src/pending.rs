//! # Module: pending
//!
//! Pure functions for parsing and mutating the `agent:pending` component body.
//!
//! Each pending item carries:
//! - a GFM task-list checkbox (`- [ ]` / `1. [ ]` or `- [x]` / `1. [x]`)
//! - an id prefix rendered as `[#xxxx]` (generated hash or caller-provided custom id)
//! - free-form text
//!
//! Canonical unordered form: `- [ ] [#a3f2] refactor preflight commit path`
//! Canonical ordered form: `1. [ ] [#a3f2] refactor preflight commit path`
//!
//! This module is I/O-free. Callers (`pending_cmd.rs`, `preflight.rs`, `write.rs`)
//! handle reading/writing files, locking, and git commits.
//!
//! ## Spec
//! - Parser accepts legacy forms and normalizes via `backfill`.
//! - IDs are stable across edits/reorders; generated once (or supplied on insert), preserved thereafter.
//! - `reap` removes `- [x]` items. `detect_reorder` diffs id order between snapshot/current.
//! - Mutation ops (`op_add/done/edit/clear/reorder`) return a new body string, never mutate in place.

use anyhow::{Result, anyhow, bail};
use std::collections::HashSet;

/// Lifecycle state for a pending item, encoded by its GFM checkbox.
///
/// - `Open` (`[ ]`) — active or not started; default for new items.
/// - `Gated` (`[/]`) — code-complete, awaiting an external gate (release,
///   telemetry, field validation). Never auto-reaped.
/// - `Done` (`[x]`) — fully complete; reaped on the next preflight cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingState {
    Open,
    Gated,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingListMarker {
    Bullet,
    Ordered(usize),
}

impl PendingListMarker {
    fn render_prefix(self, ordered_index: Option<usize>) -> String {
        match ordered_index {
            Some(index) => format!("{index}."),
            None => match self {
                PendingListMarker::Bullet => "-".to_string(),
                PendingListMarker::Ordered(index) => format!("{index}."),
            },
        }
    }

    fn is_ordered(self) -> bool {
        matches!(self, PendingListMarker::Ordered(_))
    }
}

impl PendingState {
    /// Single character for the GFM checkbox body.
    pub fn box_char(self) -> char {
        match self {
            PendingState::Open => ' ',
            PendingState::Gated => '/',
            PendingState::Done => 'x',
        }
    }

    /// Parse the inside of a `[…]` checkbox. Accepts `[X]` as `Done`.
    /// Currently only used by tests; kept on the public API for parser callers
    /// that may need to inspect a single checkbox char in isolation.
    #[allow(dead_code)]
    pub fn from_box_char(c: char) -> Option<PendingState> {
        match c {
            ' ' => Some(PendingState::Open),
            '/' => Some(PendingState::Gated),
            'x' | 'X' => Some(PendingState::Done),
            _ => None,
        }
    }
}

/// Mutating operation on a pending item — used by the state-transition matrix.
///
/// `MarkDone` is referenced by the matrix tests and reserved for the `op_done`
/// migration path (Phase 3); it is not yet wired through the CLI primitives,
/// which keep the unconditional `op_done` semantics from Phase 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum PendingOp {
    /// `[ ] → [/]` — code-complete, awaiting external gate.
    Gate,
    /// `[/] → [ ]` — return a gated item to active.
    Ungate,
    /// `[ ] | [/] → [x]` — fully complete.
    MarkDone,
}

/// Outcome of `validate_transition`. `NoOp` means "already in target state, do nothing".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionResult {
    Transition(PendingState),
    NoOp,
}

/// Apply the state-machine matrix from `specs/pending-system.md` §4.
///
/// | from \ op | Gate     | Ungate   | MarkDone |
/// |-----------|----------|----------|----------|
/// | Open      | → Gated  | error    | → Done   |
/// | Gated     | no-op    | → Open   | → Done   |
/// | Done      | error    | error    | no-op    |
///
/// Rationale (spec §4):
/// - `gate` from Done is an error: a fully-complete item cannot be re-gated; the
///   intended workflow is to add a new pending item describing the follow-up gate.
/// - `ungate` from Open or Done is an error: ungate is the inverse of gate, not
///   a generic "reset" — it requires an explicit `[/]` source.
/// - `Gate` on Gated and `MarkDone` on Done are idempotent no-ops, not errors,
///   so the granular CLI flags can be re-run safely (skill retries, watch loops).
pub fn validate_transition(from: PendingState, op: PendingOp) -> Result<TransitionResult> {
    use PendingOp::*;
    use PendingState as S;
    use TransitionResult::*;
    match (from, op) {
        (S::Open, Gate) => Ok(Transition(S::Gated)),
        (S::Open, Ungate) => bail!("cannot ungate Open item: source must be `[/]`"),
        (S::Open, MarkDone) => Ok(Transition(S::Done)),
        (S::Gated, Gate) => Ok(NoOp),
        (S::Gated, Ungate) => Ok(Transition(S::Open)),
        (S::Gated, MarkDone) => Ok(Transition(S::Done)),
        (S::Done, Gate) => {
            bail!("cannot gate Done item: add a new pending item for the follow-up gate")
        }
        (S::Done, Ungate) => bail!("cannot ungate Done item: source must be `[/]`"),
        (S::Done, MarkDone) => Ok(NoOp),
    }
}

/// A parsed pending list item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingItem {
    /// Parent list marker. `-` is the default; ordered lists store the parsed
    /// source ordinal and are renumbered canonically on render.
    pub marker: PendingListMarker,
    /// Pending item id (no `#` prefix). Generated ids are lowercase base32; custom ids
    /// and nested subtask ids may be hyphenated ASCII alphanumeric strings and are
    /// normalized to lowercase.
    pub id: String,
    /// Lifecycle state encoded by the GFM checkbox.
    pub state: PendingState,
    /// Optional typed gate (e.g., "release" for `[/release]`, "deploy" for `[/deploy]`).
    /// Only meaningful when `state == Gated`. `None` means untyped `[/]`.
    pub gate_type: Option<String>,
    /// Bullet text after the hash prefix.
    pub text: String,
    /// Raw indented continuation lines that belong to this item (for nested
    /// lists, dependency notes, etc.). Stored without the leading item line.
    pub continuation: String,
}

impl PendingItem {
    /// Render to canonical `- [<state>] [#id] text` form.
    /// Typed gates render as `[/release]`, `[/deploy]`, etc.
    pub fn render(&self) -> String {
        self.render_with_ordered_index(None)
    }

    fn render_with_ordered_index(&self, ordered_index: Option<usize>) -> String {
        let checkbox = match (&self.state, &self.gate_type) {
            (PendingState::Gated, Some(gt)) => format!("[/{}]", gt),
            _ => format!("[{}]", self.state.box_char()),
        };
        let mut out = format!(
            "{} {} [#{}] {}",
            self.marker.render_prefix(ordered_index),
            checkbox,
            self.id,
            self.text
        );
        if !self.continuation.is_empty() {
            out.push('\n');
            out.push_str(&self.continuation);
        }
        out
    }

    /// Convenience: true when state is `Done` (`[x]`).
    pub fn is_done(&self) -> bool {
        matches!(self.state, PendingState::Done)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingSegment {
    Text(String),
    Item {
        item: PendingItem,
        has_newline: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct PendingLayout {
    segments: Vec<PendingSegment>,
}

impl PendingLayout {
    fn parse(body: &str) -> Self {
        if body.is_empty() {
            return Self {
                segments: Vec::new(),
            };
        }

        let mut segments = Vec::new();
        let lines: Vec<&str> = body.split_inclusive('\n').collect();
        let mut index = 0usize;
        while index < lines.len() {
            let raw_line = lines[index];
            let has_newline = raw_line.ends_with('\n');
            let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
            if let Some(mut item) = parse_item_line(line) {
                index += 1;
                let mut continuation = String::new();
                let mut pending_blank_lines = String::new();
                while index < lines.len() {
                    let next_raw = lines[index];
                    let next_line = next_raw.strip_suffix('\n').unwrap_or(next_raw);
                    if parse_item_line(next_line).is_some() {
                        break;
                    }
                    if next_line.is_empty() {
                        pending_blank_lines.push_str(next_raw);
                        index += 1;
                        continue;
                    }
                    if is_indented_continuation_line(next_line) {
                        continuation.push_str(&pending_blank_lines);
                        pending_blank_lines.clear();
                        continuation.push_str(next_raw);
                        index += 1;
                        continue;
                    }
                    break;
                }
                item.continuation = continuation;
                segments.push(PendingSegment::Item { item, has_newline });
                if !pending_blank_lines.is_empty() {
                    segments.push(PendingSegment::Text(pending_blank_lines));
                }
            } else {
                segments.push(PendingSegment::Text(raw_line.to_string()));
                index += 1;
            }
        }
        Self { segments }
    }

    fn render(&self) -> String {
        let mut out = String::new();
        let ordered_mode = self
            .segments
            .iter()
            .filter_map(|segment| match segment {
                PendingSegment::Item { item, .. } => Some(item.marker.is_ordered()),
                PendingSegment::Text(_) => None,
            })
            .any(|is_ordered| is_ordered);
        let mut ordered_index = 1usize;
        for segment in &self.segments {
            match segment {
                PendingSegment::Text(raw) => out.push_str(raw),
                PendingSegment::Item { item, has_newline } => {
                    let render = if ordered_mode {
                        let render = item.render_with_ordered_index(Some(ordered_index));
                        ordered_index += 1;
                        render
                    } else {
                        item.render()
                    };
                    out.push_str(&render);
                    if *has_newline && item.continuation.is_empty() {
                        out.push('\n');
                    }
                }
            }
        }
        out
    }

    fn items(&self) -> Vec<PendingItem> {
        self.segments
            .iter()
            .filter_map(|segment| match segment {
                PendingSegment::Item { item, .. } => Some(item.clone()),
                PendingSegment::Text(_) => None,
            })
            .collect()
    }

    fn first_item_index(&self) -> Option<usize> {
        self.segments
            .iter()
            .position(|segment| matches!(segment, PendingSegment::Item { .. }))
    }

    fn ensure_separator_before(&mut self, index: usize) {
        if index == 0 {
            return;
        }
        match &mut self.segments[index - 1] {
            PendingSegment::Text(raw) => {
                if !raw.is_empty() && !raw.ends_with('\n') {
                    raw.push('\n');
                }
            }
            PendingSegment::Item { has_newline, .. } => {
                *has_newline = true;
            }
        }
    }

    fn insert_first_item(&mut self, item: PendingItem) {
        let index = self.first_item_index().unwrap_or(self.segments.len());
        self.insert_item_at(index, item);
    }

    /// Segment index of the item whose id matches `id` (already normalized by
    /// the caller), or `None` when no such item exists.
    fn item_segment_index(&self, id: &str) -> Option<usize> {
        self.segments.iter().position(
            |segment| matches!(segment, PendingSegment::Item { item, .. } if item.id == id),
        )
    }

    /// Segment index just past the last item (before any trailing postlude
    /// text), suitable for appending a new item at the tail of the active list.
    fn last_item_index_plus_one(&self) -> usize {
        self.segments
            .iter()
            .rposition(|segment| matches!(segment, PendingSegment::Item { .. }))
            .map(|idx| idx + 1)
            .unwrap_or_else(|| self.first_item_index().unwrap_or(self.segments.len()))
    }

    /// Insert `item` as a new segment at `index`, fixing the separator before it.
    fn insert_item_at(&mut self, index: usize, item: PendingItem) {
        self.ensure_separator_before(index);
        self.segments.insert(
            index,
            PendingSegment::Item {
                item,
                has_newline: true,
            },
        );
    }

    fn replace_items<F>(&self, mut replacer: F) -> Self
    where
        F: FnMut(&PendingItem) -> Option<PendingItem>,
    {
        let mut segments = Vec::with_capacity(self.segments.len());
        for segment in &self.segments {
            match segment {
                PendingSegment::Text(raw) => segments.push(PendingSegment::Text(raw.clone())),
                PendingSegment::Item { item, has_newline } => {
                    if let Some(next_item) = replacer(item) {
                        segments.push(PendingSegment::Item {
                            item: next_item,
                            has_newline: *has_newline,
                        });
                    }
                }
            }
        }
        Self { segments }
    }

    fn non_item_segments(&self) -> Vec<String> {
        self.segments
            .iter()
            .filter_map(|segment| match segment {
                PendingSegment::Text(raw) => Some(raw.clone()),
                PendingSegment::Item { .. } => None,
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowPendingItem {
    pub id: String,
    pub text: String,
    pub line: usize,
}

impl ShadowPendingItem {
    pub fn reference(&self) -> String {
        format!("#{} (line {})", self.id, self.line)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShadowPendingReport {
    pub duplicated_in_live_backlog: Vec<ShadowPendingItem>,
    pub shadow_only: Vec<ShadowPendingItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MalformedPendingItemLine {
    pub id: String,
    pub line: usize,
    pub text: String,
}

impl MalformedPendingItemLine {
    pub fn reference(&self) -> String {
        format!("#{} (line {}): {}", self.id, self.line, self.text)
    }
}

/// Find lines that look like tracked checklist items but are not parseable as
/// live pending items. These lines are dangerous during closeout because guards
/// that operate on parsed items would otherwise treat the matching id as absent.
pub fn detect_malformed_item_lines(body: &str) -> Vec<MalformedPendingItemLine> {
    body.lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            if parse_item_line(line).is_some() {
                return None;
            }
            let trimmed = line.trim();
            let (id, id_start) = find_valid_hash_id(trimmed)?;
            let prefix = &trimmed[..id_start];
            if !prefix_contains_task_checkbox(prefix) {
                return None;
            }
            Some(MalformedPendingItemLine {
                id: id.to_string(),
                line: idx + 1,
                text: trimmed.to_string(),
            })
        })
        .collect()
}

fn find_valid_hash_id(line: &str) -> Option<(&str, usize)> {
    let start = line.find("[#")?;
    let after = &line[start + 2..];
    let close = after.find(']')?;
    let id = &after[..close];
    is_valid_pending_id(id).then_some((id, start))
}

fn prefix_contains_task_checkbox(prefix: &str) -> bool {
    prefix.contains("[ ]")
        || prefix.contains("[/]")
        || prefix.contains("[x]")
        || prefix.contains("[X]")
        || prefix
            .split("[/")
            .nth(1)
            .and_then(|tail| tail.split(']').next())
            .is_some_and(|gate| {
                !gate.is_empty()
                    && gate
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            })
}

/// Parse the pending component body into (prelude, items, postlude).
///
/// - Prelude: leading non-list lines (whitespace, non tracked parent lines).
/// - Items: parsed tracked entries (`- ...` / `1. ...`, legacy or fully-migrated).
/// - Postlude: trailing non-list lines after the last item.
///
/// Interleaved non-item lines between backlog entries are intentionally not
/// surfaced here. Mutation helpers preserve those lines through `PendingLayout`;
/// `parse_items` remains the lossy item-only view used by reorder/diff checks.
pub fn parse_items(body: &str) -> (String, Vec<PendingItem>, String) {
    let layout = PendingLayout::parse(body);
    let Some(first_item) = layout.first_item_index() else {
        return (body.to_string(), Vec::new(), String::new());
    };
    let last_item = layout
        .segments
        .iter()
        .rposition(|segment| matches!(segment, PendingSegment::Item { .. }))
        .unwrap_or(first_item);

    let mut prelude = String::new();
    for segment in &layout.segments[..first_item] {
        if let PendingSegment::Text(raw) = segment {
            prelude.push_str(raw);
        }
    }

    let mut postlude = String::new();
    if last_item + 1 < layout.segments.len() {
        for segment in &layout.segments[last_item + 1..] {
            if let PendingSegment::Text(raw) = segment {
                postlude.push_str(raw);
            }
        }
    }

    let mut items = Vec::new();
    for segment in &layout.segments[first_item..=last_item] {
        if let PendingSegment::Item { item, .. } = segment {
            items.push(item.clone());
        }
    }

    (prelude, items, postlude)
}

/// Parse a single list item line into a `PendingItem` (id optional).
///
/// Returns `None` when the line is not a list item. When the id is missing,
/// the returned item has an empty id — callers must run `backfill` to assign one.
fn parse_item_line(line: &str) -> Option<PendingItem> {
    let (marker, rest) = parse_parent_list_marker(line)?;
    let rest = rest.trim_start();

    // Checkbox? Supports typed gates: [/release], [/deploy], etc.
    let (state, gate_type, after_box) = if let Some(r) = rest.strip_prefix("[ ]") {
        (PendingState::Open, None, r.trim_start())
    } else if let Some(r) = rest.strip_prefix("[/]") {
        (PendingState::Gated, None, r.trim_start())
    } else if let Some(inner) = rest.strip_prefix("[/") {
        // Typed gate: [/release], [/deploy], etc.
        if let Some(close) = inner.find(']') {
            let gt = &inner[..close];
            if !gt.is_empty()
                && gt
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                let r = &inner[close + 1..];
                (PendingState::Gated, Some(gt.to_lowercase()), r.trim_start())
            } else {
                (PendingState::Open, None, rest)
            }
        } else {
            (PendingState::Open, None, rest)
        }
    } else if let Some(r) = rest.strip_prefix("[x]") {
        (PendingState::Done, None, r.trim_start())
    } else if let Some(r) = rest.strip_prefix("[X]") {
        (PendingState::Done, None, r.trim_start())
    } else if let Some(r) = rest.strip_prefix("[~]") {
        // Legacy/in-progress task marker. Treat as open so an existing hash
        // immediately after it is preserved instead of re-keyed by backfill.
        (PendingState::Open, None, r.trim_start())
    } else {
        (PendingState::Open, None, rest)
    };

    // Hash id?
    let (id, text) = if let Some(after_hash) = after_box.strip_prefix("[#") {
        if let Some(close) = after_hash.find(']') {
            let id_raw = &after_hash[..close];
            let tail = after_hash[close + 1..].trim_start();
            if is_valid_pending_id(id_raw) {
                (id_raw.to_lowercase(), tail.to_string())
            } else if id_raw.is_empty() {
                // Bare [#] placeholder — consume it, text starts after ]
                (String::new(), tail.to_string())
            } else {
                (String::new(), after_box.to_string())
            }
        } else {
            (String::new(), after_box.to_string())
        }
    } else {
        (String::new(), after_box.to_string())
    };

    // Self-heal a redundant self-referential `[#id]` token embedded in the text
    // (`#pending-redundant-self-id-strip`): when an item was created with its own
    // id repeated inside the content (e.g. `[#x] [recommended] [#x] ...` from a
    // `--pending-add` whose text already carried the id), the leading `[#x]`
    // becomes the parsed id and the second `[#x]` would otherwise render as a
    // duplicate. Cross-references to *other* ids are left intact.
    let text = if id.is_empty() {
        text
    } else {
        strip_redundant_self_id_tag(&text, &id)
    };

    Some(PendingItem {
        marker,
        id,
        state,
        gate_type,
        text: text.trim_end().to_string(),
        continuation: String::new(),
    })
}

/// Remove the first bracketed `[#id]` token in `text` that equals the item's own
/// `id`, collapsing the surrounding spaces it leaves behind. Only a token that
/// matches the item's own id is removed; references to other ids stay.
fn strip_redundant_self_id_tag(text: &str, id: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut stripped = false;
    while let Some(pos) = rest.find("[#") {
        if !stripped {
            let after = &rest[pos + 2..];
            if let Some(close) = after.find(']') {
                let tok = &after[..close];
                if is_valid_pending_id(tok) && tok.eq_ignore_ascii_case(id) {
                    out.push_str(&rest[..pos]);
                    rest = &after[close + 1..];
                    stripped = true;
                    continue;
                }
            }
        }
        let keep_to = pos + 2;
        out.push_str(&rest[..keep_to]);
        rest = &rest[keep_to..];
    }
    out.push_str(rest);
    if stripped {
        collapse_inline_spaces(&out)
    } else {
        out
    }
}

/// Collapse runs of ASCII spaces/tabs to a single space and trim ends, leaving
/// newlines intact. Used after removing an inline token from a one-line item.
fn collapse_inline_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_space = false;
    for ch in text.chars() {
        if ch == ' ' || ch == '\t' {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            prev_space = false;
            out.push(ch);
        }
    }
    out.trim().to_string()
}

fn parse_parent_list_marker(line: &str) -> Option<(PendingListMarker, &str)> {
    if let Some(rest) = line.strip_prefix("- ") {
        return Some((PendingListMarker::Bullet, rest));
    }

    let digit_len = line.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digit_len == 0 {
        return None;
    }

    let (digits, tail) = line.split_at(digit_len);
    let tail = tail.strip_prefix('.')?;
    if !tail.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = tail.trim_start();
    let ordinal = digits.parse::<usize>().ok()?;
    Some((PendingListMarker::Ordered(ordinal), rest))
}

fn is_indented_continuation_line(line: &str) -> bool {
    line.starts_with(' ') || line.starts_with('\t')
}

fn is_structural_inter_item_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('#')
        || trimmed.starts_with("<!--")
        || matches!(trimmed, "---" | "***" | "___")
}

fn split_reapable_trailing_text_segment(raw: &str) -> Option<(String, String)> {
    if raw.is_empty() {
        return None;
    }

    let mut reap = String::new();
    let mut keep = String::new();
    let mut pending_blank = String::new();
    let mut reaping = true;

    for segment in raw.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        if !reaping {
            keep.push_str(segment);
            continue;
        }
        if line.trim().is_empty() {
            pending_blank.push_str(segment);
            continue;
        }
        if is_structural_inter_item_line(line) {
            keep.push_str(&pending_blank);
            pending_blank.clear();
            keep.push_str(segment);
            reaping = false;
            continue;
        }
        reap.push_str(&pending_blank);
        pending_blank.clear();
        reap.push_str(segment);
    }

    if reap.is_empty() {
        return None;
    }
    reap.push_str(&pending_blank);
    Some((reap, keep))
}

pub fn is_valid_pending_id(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

pub fn normalize_pending_id(id: &str) -> String {
    id.trim().trim_start_matches('#').to_ascii_lowercase()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeadingCustomIdPrefix {
    Explicit,
    Bracketed,
    BarePlaceholder,
}

fn detect_leading_custom_id_prefix(text: &str) -> Option<LeadingCustomIdPrefix> {
    let trimmed = text.trim_start();
    if trimmed.starts_with("id=") {
        return Some(LeadingCustomIdPrefix::Explicit);
    }
    let after_hash = trimmed.strip_prefix("[#")?;
    let close = after_hash.find(']')?;
    if close == 0 {
        Some(LeadingCustomIdPrefix::BarePlaceholder)
    } else {
        Some(LeadingCustomIdPrefix::Bracketed)
    }
}

pub(crate) fn ensure_no_leading_custom_id_prefix(text: &str, context: &str) -> Result<()> {
    match detect_leading_custom_id_prefix(text) {
        None => Ok(()),
        Some(LeadingCustomIdPrefix::BarePlaceholder) => bail!(
            "{}: bare `[#]` placeholder is invalid — omit it or use `id=<id> <text>`",
            context
        ),
        Some(LeadingCustomIdPrefix::Explicit) | Some(LeadingCustomIdPrefix::Bracketed) => bail!(
            "{}: duplicate leading custom id prefix in item text — use exactly one leading `id=<id>` or `[#id]` prefix",
            context
        ),
    }
}

pub fn ensure_no_new_leading_custom_id_prefix(
    item_id: &str,
    text: &str,
    existing_ids: &HashSet<String>,
    context: &str,
) -> Result<()> {
    match detect_leading_custom_id_prefix(text) {
        None => Ok(()),
        Some(LeadingCustomIdPrefix::BarePlaceholder) => bail!(
            "{}: bare `[#]` placeholder is invalid — omit it or use `id=<id> <text>`",
            context
        ),
        Some(LeadingCustomIdPrefix::Explicit) => bail!(
            "{}: duplicate leading custom id prefix in item text — use exactly one leading `id=<id>` or `[#id]` prefix",
            context
        ),
        Some(LeadingCustomIdPrefix::Bracketed) if existing_ids.contains(item_id) => Ok(()),
        Some(LeadingCustomIdPrefix::Bracketed) => bail!(
            "{}: duplicate leading custom id prefix in item text — use exactly one leading `id=<id>` or `[#id]` prefix",
            context
        ),
    }
}

fn extract_inline_tag_id(text: &str) -> Option<(String, String)> {
    let mut search_from = 0;
    while search_from < text.len() {
        let remaining = &text[search_from..];
        let pos = match remaining.find("[#") {
            Some(p) => p,
            None => break,
        };
        let abs_pos = search_from + pos;
        let after_open = match text.get(abs_pos + 2..) {
            Some(s) => s,
            None => {
                search_from = abs_pos + 2;
                continue;
            }
        };
        let close = match after_open.find(']') {
            Some(c) => c,
            None => {
                search_from = abs_pos + 2;
                continue;
            }
        };
        let raw_id = &after_open[..close];
        if !raw_id.is_empty() && is_valid_pending_id(raw_id) {
            let tag_end = abs_pos + 2 + close + 1;
            let before = text[..abs_pos].trim_end();
            let after = text[tag_end..].trim_start();
            let new_text = if before.is_empty() {
                after.to_string()
            } else if after.is_empty() {
                before.to_string()
            } else {
                format!("{} {}", before, after)
            };
            if !new_text.is_empty() {
                return Some((raw_id.to_lowercase(), new_text));
            }
        }
        search_from = abs_pos + 2;
    }
    None
}

fn custom_id_error(raw_id: &str) -> anyhow::Error {
    anyhow!(
        "pending add: invalid custom id `{}` — ids must be non-empty ASCII alphanumeric strings (hyphen allowed)",
        raw_id.trim()
    )
}

fn parse_explicit_custom_id_prefix(rest: &str) -> Result<(Option<String>, String)> {
    let Some((raw_id, remainder)) = rest.split_once(char::is_whitespace) else {
        bail!(
            "pending add: custom id prefix must be followed by item text (expected `id=<id> <text>`)"
        );
    };
    let custom_id = raw_id.trim().trim_start_matches('#');
    if custom_id.is_empty() {
        bail!("pending add: empty custom id after `id=` — expected `id=<id> <text>`");
    }
    if !is_valid_pending_id(custom_id) {
        return Err(custom_id_error(raw_id));
    }
    let remainder = remainder.trim();
    if remainder.is_empty() {
        bail!(
            "pending add: custom id prefix must be followed by item text (expected `id=<id> <text>`)"
        );
    }
    ensure_no_leading_custom_id_prefix(remainder, "pending add")?;
    Ok((Some(custom_id.to_lowercase()), remainder.to_string()))
}

fn parse_bracketed_custom_id_prefix(trimmed: &str) -> Result<(Option<String>, String)> {
    let Some(after_hash) = trimmed.strip_prefix("[#") else {
        return Ok((None, trimmed.to_string()));
    };
    let Some(close) = after_hash.find(']') else {
        return Ok((None, trimmed.to_string()));
    };
    let raw_id = &after_hash[..close];
    let remainder = after_hash[close + 1..].trim_start();
    if raw_id.is_empty() {
        bail!("pending add: bare `[#]` placeholder is invalid — use `id=<id> <text>` or omit it");
    }
    if !is_valid_pending_id(raw_id) {
        return Err(custom_id_error(raw_id));
    }
    if remainder.is_empty() {
        bail!(
            "pending add: bracketed custom id prefix must be followed by item text (expected `[#id] <text>`)"
        );
    }
    ensure_no_leading_custom_id_prefix(remainder, "pending add")?;
    Ok((Some(raw_id.to_lowercase()), remainder.to_string()))
}

fn parse_custom_id_prefix(text: &str) -> Result<(Option<String>, String)> {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("id=") {
        return parse_explicit_custom_id_prefix(rest);
    }
    parse_bracketed_custom_id_prefix(trimmed)
}

/// Return the explicit, caller-provided custom id from a `--pending-add` item
/// string (`id=<id> <text>` or `[#<id>] <text>`), normalized (lowercase, no
/// `#`). Returns `None` for auto-id items (no explicit prefix) and for malformed
/// prefixes — callers that need the strict parse error use the add path itself.
/// Used by mutation-time collision enforcement (#preset-item-id-collision-enforce)
/// so an explicit id that collides with a prompt preset or active item id is
/// rejected before the add is written.
pub fn explicit_custom_id(item: &str) -> Option<String> {
    match parse_custom_id_prefix(item) {
        Ok((id, _)) => id,
        Err(_) => None,
    }
}

#[test]
fn existing_item_may_keep_leading_alias_tag() {
    let mut existing_ids = HashSet::new();
    existing_ids.insert("yckq".to_string());

    ensure_no_new_leading_custom_id_prefix(
        "yckq",
        "[#ss01] ShipStation fix",
        &existing_ids,
        "pending/backlog patch",
    )
    .expect("existing alias tag should not be rejected");
}

#[test]
fn new_item_still_rejects_leading_alias_tag() {
    let existing_ids = HashSet::new();
    let err = ensure_no_new_leading_custom_id_prefix(
        "yckq",
        "[#ss01] ShipStation fix",
        &existing_ids,
        "pending/backlog patch",
    )
    .expect_err("new item alias tag should still be rejected");
    assert!(
        err.to_string()
            .contains("duplicate leading custom id prefix"),
        "unexpected error: {}",
        err
    );
}

/// Serialize items back to a body string.
#[allow(dead_code)]
pub fn render_items(prelude: &str, items: &[PendingItem], postlude: &str) -> String {
    let mut out = String::new();
    let ordered_mode = items.iter().any(|item| item.marker.is_ordered());
    let mut ordered_index = 1usize;
    out.push_str(prelude);
    if !prelude.is_empty() && !prelude.ends_with('\n') {
        out.push('\n');
    }
    for item in items {
        let render = if ordered_mode {
            let render = item.render_with_ordered_index(Some(ordered_index));
            ordered_index += 1;
            render
        } else {
            item.render()
        };
        out.push_str(&render);
        if item.continuation.is_empty() || !item.continuation.ends_with('\n') {
            out.push('\n');
        }
    }
    if !postlude.is_empty() {
        out.push_str(postlude);
        if !postlude.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// Remove a tracked item by id and return the updated body plus the removed item.
pub fn op_take_item(body: &str, id: &str) -> Result<(String, PendingItem)> {
    let id = normalize_pending_id(id);
    let layout = PendingLayout::parse(body);
    let mut taken = None;
    let rewritten = layout.replace_items(|item| {
        if item.id == id {
            taken = Some(item.clone());
            None
        } else {
            Some(item.clone())
        }
    });
    let Some(item) = taken else {
        bail!("id not found in pending list: {}", id);
    };
    Ok((rewritten.render(), item))
}

/// Remove every item whose id matches `id`, returning the rewritten body and
/// the removed items. Unlike [`op_take_item`] (which keeps a single
/// representative and errors on a miss), this collapses duplicate-id entries in
/// one pass and returns them all — used by `--review-remove` so a duplicated
/// review id (e.g. an interleaved finalize that wrote the same `#id` twice) can
/// be cleared without an ambiguous edit-by-id.
pub fn op_take_all_by_id(body: &str, id: &str) -> (String, Vec<PendingItem>) {
    let id = normalize_pending_id(id);
    let layout = PendingLayout::parse(body);
    let mut taken = Vec::new();
    let rewritten = layout.replace_items(|item| {
        if item.id == id {
            taken.push(item.clone());
            None
        } else {
            Some(item.clone())
        }
    });
    (rewritten.render(), taken)
}

/// Collapse runs of identical same-id entries to a single representative,
/// preserving first-seen order. Two items are "identical" when their id, state,
/// gate type, text, and continuation all match — this targets the exact-dup
/// shape an interleaved finalize produces, never distinct items that merely
/// share an id (those remain so an ambiguity warning still surfaces). Returns
/// the rewritten body and the ids that had duplicates removed.
pub fn op_dedupe_identical_items(body: &str) -> (String, Vec<String>) {
    let layout = PendingLayout::parse(body);
    let mut seen: Vec<PendingItem> = Vec::new();
    let mut deduped_ids: Vec<String> = Vec::new();
    let rewritten = layout.replace_items(|item| {
        if seen.iter().any(|prev| prev == item) {
            if !deduped_ids.contains(&item.id) {
                deduped_ids.push(item.id.clone());
            }
            None
        } else {
            seen.push(item.clone());
            Some(item.clone())
        }
    });
    (rewritten.render(), deduped_ids)
}

/// Insert an existing tracked item at the first item slot of a component body.
pub fn op_insert_item_first(body: &str, item: PendingItem) -> String {
    let mut layout = PendingLayout::parse(body);
    layout.insert_first_item(item);
    layout.render()
}

/// Append existing tracked items after the current item list, preserving order.
pub fn op_append_items(body: &str, items: &[PendingItem]) -> String {
    let mut layout = PendingLayout::parse(body);
    let insert_at = layout
        .segments
        .iter()
        .rposition(|segment| matches!(segment, PendingSegment::Item { .. }))
        .map(|idx| idx + 1)
        .unwrap_or_else(|| layout.first_item_index().unwrap_or(layout.segments.len()));
    layout.ensure_separator_before(insert_at);
    for (offset, item) in items.iter().enumerate() {
        layout.segments.insert(
            insert_at + offset,
            PendingSegment::Item {
                item: item.clone(),
                has_newline: true,
            },
        );
    }
    layout.render()
}

/// Extract all items matching `state`, returning the updated body and removed items.
pub fn op_take_items_by_state(body: &str, state: PendingState) -> (String, Vec<PendingItem>) {
    let layout = PendingLayout::parse(body);
    let mut taken = Vec::new();
    let rewritten = layout.replace_items(|item| {
        if item.state == state {
            taken.push(item.clone());
            None
        } else {
            Some(item.clone())
        }
    });
    (rewritten.render(), taken)
}

/// Extract non-done items with ids in `ids`, returning the updated body and removed items.
pub fn op_take_active_items_by_ids(
    body: &str,
    ids: &HashSet<String>,
) -> (String, Vec<PendingItem>) {
    if ids.is_empty() {
        return (body.to_string(), Vec::new());
    }
    let ids: HashSet<String> = ids.iter().map(|id| normalize_pending_id(id)).collect();
    let layout = PendingLayout::parse(body);
    let mut taken = Vec::new();
    let rewritten = layout.replace_items(|item| {
        if !item.is_done() && ids.contains(&item.id) {
            taken.push(item.clone());
            None
        } else {
            Some(item.clone())
        }
    });
    (rewritten.render(), taken)
}

pub fn canonicalize_preserving_non_item_lines(body: &str) -> String {
    PendingLayout::parse(body).render()
}

fn parse_pending_edit_payload(new_text: &str) -> Result<(String, String)> {
    let synthetic = format!("- [ ] [#edit] {}", new_text);
    let layout = PendingLayout::parse(&synthetic);
    let items = layout.items();
    if items.len() != 1 {
        bail!(
            "pending edit: multiline text may only describe one parent item; use indented continuation lines for child subtasks"
        );
    }
    if layout
        .non_item_segments()
        .into_iter()
        .any(|segment| !segment.trim().is_empty())
    {
        bail!(
            "pending edit: multiline text may only contain indented continuation lines after the first line"
        );
    }
    let mut item = items.into_iter().next().expect("single parsed edit item");
    if !item.continuation.is_empty() && !item.continuation.ends_with('\n') {
        item.continuation.push('\n');
    }
    Ok((item.text, item.continuation))
}

fn trim_boundary_blank_segments(mut segments: Vec<String>) -> Vec<String> {
    while matches!(segments.first(), Some(segment) if segment.trim().is_empty()) {
        segments.remove(0);
    }
    while matches!(segments.last(), Some(segment) if segment.trim().is_empty()) {
        segments.pop();
    }
    segments
}

pub fn preserves_non_item_structure(lhs: &str, rhs: &str) -> bool {
    trim_boundary_blank_segments(PendingLayout::parse(lhs).non_item_segments())
        == trim_boundary_blank_segments(PendingLayout::parse(rhs).non_item_segments())
}

pub fn merge_partial_backlog_prefix(current_body: &str, target_body: &str) -> Option<String> {
    let current = PendingLayout::parse(current_body);
    let target = PendingLayout::parse(target_body);

    let current_texts: Vec<(usize, String)> = current
        .segments
        .iter()
        .enumerate()
        .filter_map(|(idx, segment)| match segment {
            PendingSegment::Text(raw) if !raw.trim().is_empty() => Some((idx, raw.clone())),
            _ => None,
        })
        .collect();
    let target_texts: Vec<String> = target
        .segments
        .iter()
        .filter_map(|segment| match segment {
            PendingSegment::Text(raw) if !raw.trim().is_empty() => Some(raw.clone()),
            _ => None,
        })
        .collect();

    if current_texts.is_empty()
        || target_texts.is_empty()
        || target_texts.len() >= current_texts.len()
        || !target_texts
            .iter()
            .zip(current_texts.iter())
            .all(|(target_text, (_, current_text))| target_text == current_text)
    {
        return None;
    }

    let tail_start = current_texts[target_texts.len()].0;
    let mut segments = target.segments.clone();
    segments.extend(current.segments[tail_start..].iter().cloned());
    Some(PendingLayout { segments }.render())
}

pub fn detect_shadow_open_items(doc: &str) -> Result<ShadowPendingReport> {
    let components = crate::component::parse(doc)?;
    let Some(backlog_component) = components
        .iter()
        .find(|component| crate::component::is_backlog_component(&component.name))
    else {
        return Ok(ShadowPendingReport::default());
    };

    let (_, live_items, _) = parse_items(backlog_component.content(doc));
    let live_open_ids: HashSet<String> = live_items
        .into_iter()
        .filter(|item| !item.id.is_empty() && !item.is_done())
        .map(|item| item.id)
        .collect();

    let excluded_ranges: Vec<(usize, usize)> = components
        .iter()
        .filter(|component| {
            crate::component::is_tracked_work_component(&component.name)
                || component.name == "exchange"
        })
        .map(|component| (component.open_start, component.close_end))
        .collect();
    let code_ranges = crate::component::find_code_ranges(doc);

    let mut report = ShadowPendingReport::default();
    let mut seen_ids = HashSet::new();
    let mut offset = 0usize;

    for (line_idx, raw_line) in doc.split_inclusive('\n').enumerate() {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let line_start = offset;
        let line_end = offset + raw_line.len();
        offset = line_end;

        if excluded_ranges
            .iter()
            .any(|(start, end)| line_start < *end && line_end > *start)
        {
            continue;
        }
        if code_ranges
            .iter()
            .any(|(start, end)| line_start < *end && line_end > *start)
        {
            continue;
        }

        let Some(item) = parse_item_line(line) else {
            continue;
        };
        if item.id.is_empty() || item.is_done() || !seen_ids.insert(item.id.clone()) {
            continue;
        }

        let shadow = ShadowPendingItem {
            id: item.id.clone(),
            text: item.text,
            line: line_idx + 1,
        };
        if live_open_ids.contains(&item.id) {
            report.duplicated_in_live_backlog.push(shadow);
        } else {
            report.shadow_only.push(shadow);
        }
    }

    Ok(report)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedBacklogItem {
    pub id: String,
    pub text: String,
}

impl DroppedBacklogItem {
    pub fn reference(&self) -> String {
        format!("#{}", self.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DroppedBacklogReport {
    pub dropped: Vec<DroppedBacklogItem>,
}

/// Compare current document against a baseline to find open backlog items that
/// existed in the baseline but are completely absent from the current document.
///
/// "Completely absent" means the item's ID does not appear in:
/// - the live `agent:backlog` (any state: open, gated, or done)
/// - the `agent:icebox` component
/// - shadow/commented sections outside the live backlog
///
/// Items found in shadow sections are NOT reported here — the shadow guard
/// (`detect_shadow_open_items`) handles those separately.
///
/// `done_ids` allows callers to exclude items that were explicitly marked done
/// during the current cycle (from `cycle_state.pending_done_ids`).
#[allow(dead_code)]
pub fn detect_dropped_from_history(
    current_doc: &str,
    baseline_doc: &str,
    done_ids: &HashSet<String>,
) -> Result<DroppedBacklogReport> {
    detect_dropped_from_history_with_extra_current_ids(
        current_doc,
        baseline_doc,
        done_ids,
        &HashSet::new(),
    )
}

pub fn detect_dropped_from_history_with_extra_current_ids(
    current_doc: &str,
    baseline_doc: &str,
    done_ids: &HashSet<String>,
    extra_current_ids: &HashSet<String>,
) -> Result<DroppedBacklogReport> {
    let baseline_components = crate::component::parse(baseline_doc)?;
    let Some(baseline_backlog) = baseline_components
        .iter()
        .find(|c| crate::component::is_backlog_component(&c.name))
    else {
        return Ok(DroppedBacklogReport::default());
    };

    let (_, baseline_items, _) = parse_items(baseline_backlog.content(baseline_doc));
    let baseline_open: Vec<(String, String)> = baseline_items
        .into_iter()
        .filter(|item| !item.id.is_empty() && !item.is_done())
        .map(|item| (item.id, item.text))
        .collect();

    if baseline_open.is_empty() {
        return Ok(DroppedBacklogReport::default());
    }

    let current_components = crate::component::parse(current_doc)?;

    let mut current_ids: HashSet<String> = HashSet::new();

    for comp in &current_components {
        if crate::component::is_tracked_work_component(&comp.name) {
            let (_, items, _) = parse_items(comp.content(current_doc));
            for item in items {
                if !item.id.is_empty() {
                    current_ids.insert(item.id);
                }
            }
        } else if crate::component::is_backlog_done_component(&comp.name) {
            current_ids.extend(extract_pending_ids_from_text(comp.content(current_doc)));
        }
    }
    current_ids.extend(extra_current_ids.iter().cloned());

    let excluded_ranges: Vec<(usize, usize)> = current_components
        .iter()
        .filter(|c| crate::component::is_tracked_work_component(&c.name))
        .map(|c| (c.open_start, c.close_end))
        .collect();
    let code_ranges = crate::component::find_code_ranges(current_doc);

    let mut offset = 0usize;
    for raw_line in current_doc.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let line_start = offset;
        let line_end = offset + raw_line.len();
        offset = line_end;

        if excluded_ranges
            .iter()
            .any(|(start, end)| line_start < *end && line_end > *start)
        {
            continue;
        }
        if code_ranges
            .iter()
            .any(|(start, end)| line_start < *end && line_end > *start)
        {
            continue;
        }

        if let Some(item) = parse_item_line(line)
            && !item.id.is_empty()
        {
            current_ids.insert(item.id);
        }
    }

    let mut dropped = Vec::new();
    for (id, text) in baseline_open {
        if done_ids.contains(&id) {
            continue;
        }
        if !current_ids.contains(&id) {
            dropped.push(DroppedBacklogItem { id, text });
        }
    }

    Ok(DroppedBacklogReport { dropped })
}

/// Active (open `[ ]`, not gated, not done) backlog item ids in document order.
///
/// Used by the `queue` attribute on `agent:backlog` / `agent:icebox` to
/// regenerate `agent:queue` `do [#id]` prompts (`#backlog-queue-sync-attr`).
/// Gated (`[/]`) and done (`[x]`) items are excluded — they are not actionable
/// queue targets. Items without an id are skipped.
pub fn active_item_ids(body: &str) -> Vec<String> {
    PendingLayout::parse(body)
        .items()
        .into_iter()
        .filter(|item| matches!(item.state, PendingState::Open) && !item.id.is_empty())
        .map(|item| item.id.clone())
        .collect()
}

fn item_has_enqueue_marker(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains(":inbox_tray:") || lower.split_whitespace().any(token_is_enqueue_marker)
}

fn token_is_enqueue_marker(token: &str) -> bool {
    let trimmed =
        token.trim_matches(|ch: char| matches!(ch, '[' | ']' | '(' | ')' | ':' | ',' | '.' | ';'));
    matches!(
        trimmed,
        "/enqueue" | "**enqueue**" | "__enqueue__" | "*enqueue*" | "_enqueue_" | "`enqueue`"
    )
}

/// Active open backlog item ids marked for one-shot queue insertion.
///
/// `:inbox_tray:`, `/enqueue`, and Markdown-decorated `enqueue` markers let
/// an item opt into queue population without requiring the whole component to
/// carry the `queue` attribute.
pub fn active_enqueue_item_ids(body: &str) -> Vec<String> {
    PendingLayout::parse(body)
        .items()
        .into_iter()
        .filter(|item| matches!(item.state, PendingState::Open) && !item.id.is_empty())
        .filter(|item| item_has_enqueue_marker(&item.text))
        .map(|item| item.id.clone())
        .collect()
}

/// Rank an item's priority from a `priority=<1..9>` token anywhere in its text
/// (`#backlog-priority-attribute`). `1` is the highest priority and sorts first;
/// `9` is the lowest numbered priority. An item with no valid `priority=` token
/// ranks `10` — below every numbered item — so unprioritized items sort last
/// while preserving their authored relative order under a stable sort.
pub const PRIORITY_RANK_UNSET: u8 = 10;

pub fn item_priority_rank(text: &str) -> u8 {
    for token in text.split_whitespace() {
        if let Some(value) = token.strip_prefix("priority=")
            && let Ok(n) = value.parse::<u8>()
            && (1..=9).contains(&n)
        {
            return n;
        }
    }
    PRIORITY_RANK_UNSET
}

/// Active (open) backlog item ids paired with their priority rank, in document
/// order. Used to priority-order a synced `agent:queue` (`#backlog-priority-attribute`).
pub fn active_item_priorities(body: &str) -> Vec<(String, u8)> {
    PendingLayout::parse(body)
        .items()
        .into_iter()
        .filter(|item| matches!(item.state, PendingState::Open) && !item.id.is_empty())
        .map(|item| (item.id.clone(), item_priority_rank(&item.text)))
        .collect()
}

/// Parse `after=#id` dependency tokens from an item's text
/// (`#queue-auto-dag-priority`). Declares that this item must be ordered *after*
/// each named id. Accepts repeated tokens and comma lists: `after=#a`,
/// `after=#a,#b`, `after=#a after=#b`. Ids are normalized lowercase with
/// `#`/`[`/`]` stripped; only valid pending ids are kept. The `after=` token must
/// start at a word boundary so prose like `hereafter=` does not match.
pub fn item_after_deps(text: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let bytes = text.as_bytes();
    let mut search = 0;
    while let Some(rel) = text[search..].find("after=") {
        let idx = search + rel;
        search = idx + "after=".len();
        if idx > 0 && bytes[idx - 1].is_ascii_alphanumeric() {
            continue;
        }
        let value = &text[search..];
        let chunk: String = value
            .chars()
            .take_while(|c| {
                c.is_ascii_alphanumeric() || matches!(c, '#' | '-' | '_' | ',' | '[' | ']')
            })
            .collect();
        for part in chunk.split(',') {
            let id = part
                .trim()
                .trim_matches(|c| matches!(c, '#' | '[' | ']'))
                .to_ascii_lowercase();
            if !id.is_empty() && is_valid_pending_id(&id) {
                deps.push(id);
            }
        }
    }
    deps
}

/// Active (open) backlog item ids paired with their `after=#id` dependency ids,
/// in document order. Feeds the auto-dag queue ordering (`#queue-auto-dag-priority`).
pub fn active_item_after_deps(body: &str) -> Vec<(String, Vec<String>)> {
    PendingLayout::parse(body)
        .items()
        .into_iter()
        .filter(|item| matches!(item.state, PendingState::Open) && !item.id.is_empty())
        .map(|item| (item.id.clone(), item_after_deps(&item.text)))
        .collect()
}

/// Stable-sort the item lines of a pending body by per-item priority
/// (`#backlog-priority-attribute`), preserving every non-item segment (blank
/// lines, prose, ordered-list separators) at its original position. Returns
/// `Some(new_body)` when the order changes, `None` otherwise (idempotent).
pub fn sort_by_priority(body: &str) -> Option<String> {
    let layout = PendingLayout::parse(body);
    let positions: Vec<usize> = layout
        .segments
        .iter()
        .enumerate()
        .filter_map(|(i, s)| matches!(s, PendingSegment::Item { .. }).then_some(i))
        .collect();
    if positions.len() < 2 {
        return None;
    }
    let mut slots: Vec<(usize, PendingItem)> = positions
        .iter()
        .enumerate()
        .map(|(slot, &pos)| match &layout.segments[pos] {
            PendingSegment::Item { item, .. } => (slot, item.clone()),
            _ => unreachable!(),
        })
        .collect();
    let before: Vec<usize> = slots.iter().map(|(slot, _)| *slot).collect();
    slots.sort_by_key(|(_, item)| item_priority_rank(&item.text));
    let after: Vec<usize> = slots.iter().map(|(slot, _)| *slot).collect();
    if before == after {
        return None;
    }
    let mut segments = layout.segments.clone();
    for (target, &pos) in positions.iter().enumerate() {
        let item = slots[target].1.clone();
        if let PendingSegment::Item { has_newline, .. } = layout.segments[pos] {
            segments[pos] = PendingSegment::Item { item, has_newline };
        }
    }
    Some(PendingLayout { segments }.render())
}

pub fn extract_pending_ids_from_text(text: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    let mut rest = text;
    while let Some(start) = rest.find("[#") {
        let after = &rest[start + 2..];
        let Some(end) = after.find(']') else {
            break;
        };
        let id = &after[..end];
        if is_valid_pending_id(id) {
            ids.insert(id.to_ascii_lowercase());
        }
        rest = &after[end + 1..];
    }
    ids
}

/// Generate a stable 4-char base32 hash from `(text, doc_id, counter)`.
///
/// Backward-compat thin wrapper over [`generate_hash_n`] at width 4. Existing
/// docs keep their 4-char IDs on re-backfill because width-4 output is
/// bit-identical to the original formula. Kept in the public API for
/// backward compatibility and as the canonical entry point for width-4
/// hashing in tests.
#[allow(dead_code)]
pub fn generate_hash(text: &str, doc_id: &str, counter: u64) -> String {
    generate_hash_n(text, doc_id, counter, 4)
}

/// Generate a stable variable-width base32 hash. `width` is clamped to `[4, 8]`
/// — the spec §1 ceiling on collision extension.
///
/// Width-4 output is bit-identical to the pre-#14z4 `generate_hash` formula,
/// so lazy backfill on existing docs is a no-op.
pub fn generate_hash_n(text: &str, doc_id: &str, counter: u64, width: usize) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hasher.update(b":");
    hasher.update(doc_id.as_bytes());
    hasher.update(b":");
    hasher.update(counter.to_le_bytes());
    let digest = hasher.finalize();

    // Base32 lowercase alphabet (no 0/1/8/9 per crockford would complicate collisions;
    // stick with full a-z/0-9 subset for simplicity).
    const ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz"; // 32 chars

    let width = width.clamp(4, 8);
    let mut out = String::with_capacity(width);

    // First 4 chars: preserve the original bit packing so width-4 output is
    // stable across the #14z4 refactor. (bottom 20 bits of b0<<16 | b1<<8 | b2)
    let b0 = digest[0] as u32;
    let b1 = digest[1] as u32;
    let b2 = digest[2] as u32;
    let v: u32 = (b0 << 16) | (b1 << 8) | b2;
    out.push(ALPHABET[((v >> 15) & 0x1f) as usize] as char);
    out.push(ALPHABET[((v >> 10) & 0x1f) as usize] as char);
    out.push(ALPHABET[((v >> 5) & 0x1f) as usize] as char);
    out.push(ALPHABET[(v & 0x1f) as usize] as char);

    // Extra chars (5..=8): draw from bytes 3..=5 of the digest. Same 5-bit
    // packing layout, starting from a fresh 24-bit window so layer N+1 is not
    // a mechanical continuation of layer N (prevents widening from aliasing
    // to a near-neighbor that also collides).
    if width > 4 {
        let e0 = digest[3] as u32;
        let e1 = digest[4] as u32;
        let e2 = digest[5] as u32;
        let extra: u32 = (e0 << 16) | (e1 << 8) | e2;
        for i in 0..(width - 4) {
            let shift = 15 - (i as u32) * 5;
            out.push(ALPHABET[((extra >> shift) & 0x1f) as usize] as char);
        }
    }
    out
}

/// Assign a hash id that does not collide with `taken`.
///
/// Starts at width 4 and extends up to the spec §1 ceiling of 8. Counter
/// cycles within each width before widening — at width 4 that's ~1M values
/// before we touch width 5, so normal docs never widen in practice.
fn assign_unique_hash(text: &str, doc_id: &str, taken: &HashSet<String>) -> String {
    // Per-width retry budget: small because a single widening step gives
    // another 5 bits of entropy, which is a much bigger win than continuing
    // to spin at the old width.
    const RETRIES_PER_WIDTH: u64 = 4;
    let mut counter: u64 = 0;
    loop {
        // width = 4 + (counter / 4), clamped at 8 (spec §1 ceiling).
        // Once we hit width 8, keep spinning counter forever — at 40 bits of
        // entropy a further collision is effectively impossible in practice.
        let width = std::cmp::min(4 + (counter / RETRIES_PER_WIDTH) as usize, 8);
        let id = generate_hash_n(text, doc_id, counter, width);
        if !taken.contains(&id) {
            return id;
        }
        counter = counter.saturating_add(1);
    }
}

/// Assign a nested subtask id using the parent id as a visible prefix.
///
/// Shape: `<parent_id>-<suffix>`, where `suffix` is a stable 4..=8 char hash.
fn assign_unique_child_hash(
    parent_id: &str,
    text: &str,
    doc_id: &str,
    taken: &HashSet<String>,
) -> String {
    const RETRIES_PER_WIDTH: u64 = 4;
    let seed = format!("{parent_id}:{text}");
    let mut counter: u64 = 0;
    loop {
        let width = std::cmp::min(4 + (counter / RETRIES_PER_WIDTH) as usize, 8);
        let suffix = generate_hash_n(&seed, doc_id, counter, width);
        let candidate = format!("{parent_id}-{suffix}");
        if !taken.contains(&candidate) {
            return candidate;
        }
        counter = counter.saturating_add(1);
    }
}

fn split_indent(line: &str) -> (&str, &str) {
    let idx = line
        .char_indices()
        .find_map(|(idx, ch)| (!ch.is_whitespace()).then_some(idx))
        .unwrap_or(line.len());
    line.split_at(idx)
}

fn normalize_nested_subtasks(
    continuation: &str,
    parent_id: &str,
    doc_id: &str,
    taken: &mut HashSet<String>,
) -> (String, bool) {
    if continuation.is_empty() {
        return (String::new(), false);
    }

    let mut changed = false;
    let mut out = String::with_capacity(continuation.len());
    for raw_line in continuation.split_inclusive('\n') {
        let has_newline = raw_line.ends_with('\n');
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        if !is_indented_continuation_line(line) {
            out.push_str(raw_line);
            continue;
        }

        let (indent, trimmed) = split_indent(line);
        let Some(mut child) = parse_item_line(trimmed) else {
            out.push_str(raw_line);
            continue;
        };

        let duplicate_existing_id = !child.id.is_empty() && taken.contains(&child.id);
        if child.id.is_empty() || duplicate_existing_id {
            child.id = assign_unique_child_hash(parent_id, &child.text, doc_id, taken);
            taken.insert(child.id.clone());
            changed = true;
        } else {
            taken.insert(child.id.clone());
        }

        let rendered = child.render();
        if rendered != trimmed {
            changed = true;
        }
        out.push_str(indent);
        out.push_str(&rendered);
        if has_newline {
            out.push('\n');
        }
    }
    (out, changed)
}

/// Lazy backfill: ensure every item has a hash id and a checkbox.
///
/// - Items missing a hash get a new one (guaranteed unique within the component).
/// - Checkboxes are normalized (default `[ ]`).
/// - Returns `(new_body, changed)`. `changed = false` when the body was already canonical.
pub fn backfill(body: &str, doc_id: &str, existing_ids: &HashSet<String>) -> (String, bool) {
    let layout = PendingLayout::parse(body);
    let items = layout.items();
    let mut taken: HashSet<String> = existing_ids.clone();
    for item in &items {
        if !item.id.is_empty() {
            taken.insert(item.id.clone());
        }
    }

    let mut changed = false;
    let rewritten = layout.replace_items(|item| {
        // Drop content-less items: a bullet with no description text and no
        // continuation carries no information (typically a stray empty `- [ ]`
        // from an editor/IPC insertion). Backfilling it would manufacture a
        // phantom `[#hash]` id for an empty line — the "description disappeared"
        // bug (#icebox-empty-item-phantom-id) — so remove it instead. This also
        // self-heals an already-cemented id-only empty item (`- [ ] [#hash]`).
        if item.text.trim().is_empty() && item.continuation.trim().is_empty() {
            changed = true;
            return None;
        }
        let mut next = item.clone();
        if next.id.is_empty() {
            let id = assign_unique_hash(&next.text, doc_id, &taken);
            taken.insert(id.clone());
            next.id = id;
            changed = true;
        }
        let (continuation, nested_changed) =
            normalize_nested_subtasks(&next.continuation, &next.id, doc_id, &mut taken);
        if nested_changed {
            next.continuation = continuation;
            changed = true;
        }
        Some(next)
    });

    let new_body = rewritten.render();

    // Also mark as changed when the canonical render differs from the input
    // (e.g., legacy whitespace / missing checkbox normalization).
    if new_body != body {
        changed = true;
    }
    (new_body, changed)
}

/// Reap `[x]` items. `[/]` (gated) items are never reaped.
/// Returns `(new_body, removed_ids)`.
#[allow(dead_code)]
pub fn reap(body: &str) -> Result<(String, Vec<String>)> {
    let (new_body, removed) = reap_with_items(body)?;
    let ids = removed.iter().map(|i| i.id.clone()).collect();
    Ok((new_body, ids))
}

/// Reap `[x]` items and return the removed items (with text), not just ids.
/// Used by preflight to archive reaped items to an `agent:done` component.
pub fn reap_with_items(body: &str) -> Result<(String, Vec<PendingItem>)> {
    let layout = PendingLayout::parse(body);
    let items = layout.items();
    let missing_done: Vec<&PendingItem> = items
        .iter()
        .filter(|item| item.is_done() && item.id.is_empty())
        .collect();
    if !missing_done.is_empty() {
        let refs = missing_done
            .iter()
            .map(|item| format!("\"{}\"", item.text))
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "pending reap requires ids for completed items; run backfill first (missing ids on done item(s): {})",
            refs
        );
    }
    let mut removed = Vec::new();
    let mut segments = Vec::with_capacity(layout.segments.len());
    let mut index = 0usize;
    while index < layout.segments.len() {
        match &layout.segments[index] {
            PendingSegment::Text(raw) => {
                segments.push(PendingSegment::Text(raw.clone()));
                index += 1;
            }
            PendingSegment::Item { item, has_newline } => {
                if !item.is_done() {
                    segments.push(PendingSegment::Item {
                        item: item.clone(),
                        has_newline: *has_newline,
                    });
                    index += 1;
                    continue;
                }

                let mut removed_item = item.clone();
                let mut next_index = index + 1;
                while let Some(PendingSegment::Text(raw)) = layout.segments.get(next_index) {
                    if let Some((reap_text, keep_text)) = split_reapable_trailing_text_segment(raw)
                    {
                        removed_item.continuation.push_str(&reap_text);
                        next_index += 1;
                        if !keep_text.is_empty() {
                            segments.push(PendingSegment::Text(keep_text));
                            break;
                        }
                    } else {
                        segments.push(PendingSegment::Text(raw.clone()));
                        next_index += 1;
                        break;
                    }
                }
                if !removed_item.id.is_empty() {
                    removed.push(removed_item);
                }
                index = next_index;
            }
        }
    }
    let new_layout = PendingLayout { segments };
    if removed.is_empty() {
        return Ok((body.to_string(), removed));
    }
    let new_body = new_layout.render();
    Ok((new_body, removed))
}

/// Remove specific tracked items by id while preserving surrounding non-item
/// structure in the source body. Returns `(remaining_body, moved_body, matched_ids)`.
pub fn extract_items_by_id(body: &str, ids: &[String]) -> Result<(String, String, Vec<String>)> {
    let requested: Vec<String> = ids.iter().map(|id| id.trim().to_lowercase()).collect();
    let mut matched_ids = Vec::new();
    let mut moved_items = Vec::new();
    let remaining = PendingLayout::parse(body).replace_items(|item| {
        if requested.contains(&item.id) {
            matched_ids.push(item.id.clone());
            moved_items.push(item.clone());
            None
        } else {
            Some(item.clone())
        }
    });
    let moved_body = render_items("", &moved_items, "");
    Ok((remaining.render(), moved_body, matched_ids))
}

/// Detect reorder: returns `Some(current_order)` when id-sets match but order differs.
/// Returns `None` when id-sets differ or order is identical.
pub fn detect_reorder(snapshot_body: &str, current_body: &str) -> Option<Vec<String>> {
    let (_, snap_items, _) = parse_items(snapshot_body);
    let (_, cur_items, _) = parse_items(current_body);

    let snap_ids: Vec<String> = snap_items
        .iter()
        .filter(|i| !i.id.is_empty())
        .map(|i| i.id.clone())
        .collect();
    let cur_ids: Vec<String> = cur_items
        .iter()
        .filter(|i| !i.id.is_empty())
        .map(|i| i.id.clone())
        .collect();

    if snap_ids.len() != cur_ids.len() {
        return None;
    }
    let snap_set: HashSet<&String> = snap_ids.iter().collect();
    let cur_set: HashSet<&String> = cur_ids.iter().collect();
    if snap_set != cur_set {
        return None;
    }
    if snap_ids == cur_ids {
        return None;
    }
    Some(cur_ids)
}

/// Insert a new item at the beginning of the body. Binary assigns hash and `[ ]`
/// (or `[/]` if `gated`).
/// Returns `(new_body, assigned_id)`.
/// `#ah0s`: explicit insertion position for a newly added pending item. The
/// backlog is a priority-ordered pool with id-based consumption (not a stack or
/// queue), so position encodes author intent and is set explicitly when it
/// matters. `First` is the cheap default used by `--pending-add`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddPosition<'a> {
    /// Insert at the front of the active list (default).
    First,
    /// Append at the tail of the active list (before any trailing text).
    Last,
    /// Insert immediately after the existing item with this id.
    After(&'a str),
    /// Insert immediately before the existing item with this id.
    Before(&'a str),
}

pub fn op_add(body: &str, text: &str, doc_id: &str, gated: bool) -> Result<(String, String)> {
    op_add_at(body, text, doc_id, gated, AddPosition::First)
}

/// Position-aware variant of [`op_add`] (`#ah0s`). Assigns/validates the id and
/// dedups exactly like `op_add`, then inserts the new item at `position`.
/// `After`/`Before` error if the anchor id is absent.
pub fn op_add_at(
    body: &str,
    text: &str,
    doc_id: &str,
    gated: bool,
    position: AddPosition<'_>,
) -> Result<(String, String)> {
    let (custom_id, text) = parse_custom_id_prefix(text)?;
    let mut text = text.trim().to_string();
    if text.is_empty() {
        bail!("pending add: text must be non-empty");
    }
    if text.starts_with("[ ]")
        || text.starts_with("[/]")
        || text.starts_with("[x]")
        || text.starts_with("[X]")
    {
        bail!(
            "pending add: text must not start with a state marker ([ ], [/], [x]); use --pending-add-gated for gated items"
        );
    }

    let mut inline_custom_id = None;
    if custom_id.is_none()
        && let Some((tag_id, cleaned)) = extract_inline_tag_id(&text)
    {
        inline_custom_id = Some(tag_id);
        text = cleaned;
    }

    let mut layout = PendingLayout::parse(body);
    let items = layout.items();

    // Dedup: reject if an item with identical text already exists.
    if items.iter().any(|i| i.text == text) {
        bail!("pending add: duplicate item text already exists: {}", text);
    }

    let mut taken: HashSet<String> = items
        .iter()
        .filter(|i| !i.id.is_empty())
        .map(|i| i.id.clone())
        .collect();

    let id = if let Some(custom_id) = custom_id {
        if taken.contains(&custom_id) {
            bail!("pending add: custom id already exists: {}", custom_id);
        }
        custom_id
    } else if let Some(inline_id) = inline_custom_id {
        if taken.contains(&inline_id) {
            bail!("pending add: inline tag id already exists: {}", inline_id);
        }
        inline_id
    } else {
        assign_unique_hash(&text, doc_id, &taken)
    };
    taken.insert(id.clone());

    let new_item = PendingItem {
        marker: PendingListMarker::Bullet,
        id: id.clone(),
        state: if gated {
            PendingState::Gated
        } else {
            PendingState::Open
        },
        gate_type: None,
        text,
        continuation: String::new(),
    };
    match position {
        AddPosition::First => layout.insert_first_item(new_item),
        AddPosition::Last => {
            let index = layout.last_item_index_plus_one();
            layout.insert_item_at(index, new_item);
        }
        AddPosition::After(anchor) => {
            let anchor = normalize_pending_id(anchor);
            let Some(idx) = layout.item_segment_index(&anchor) else {
                bail!("pending add: anchor id not found for --pending-add-after: {anchor}");
            };
            layout.insert_item_at(idx + 1, new_item);
        }
        AddPosition::Before(anchor) => {
            let anchor = normalize_pending_id(anchor);
            let Some(idx) = layout.item_segment_index(&anchor) else {
                bail!("pending add: anchor id not found for --pending-add-before: {anchor}");
            };
            layout.insert_item_at(idx, new_item);
        }
    }
    Ok((layout.render(), id))
}

/// Mark an item `[x]` by id. Phase 1: state-machine validation lives in
/// the upcoming `pending_cmd` layer; this primitive forces Done unconditionally.
pub fn op_done(body: &str, id: &str) -> Result<String> {
    let id = normalize_pending_id(id);
    let mut found = false;
    let layout = PendingLayout::parse(body).replace_items(|item| {
        if item.id == id {
            found = true;
            let mut next = item.clone();
            next.state = PendingState::Done;
            next.gate_type = None;
            Some(next)
        } else {
            Some(item.clone())
        }
    });
    if !found {
        return Err(anyhow!("pending done: no item with id [#{}]", id));
    }
    Ok(layout.render())
}

/// Transition an item to `Gated` (`[/]`) by id.
///
/// - `Open → Gated`: state mutates.
/// - `Gated → Gated`: idempotent no-op (returns body unchanged).
/// - `Done → *`: error (cannot re-gate a completed item).
pub fn op_gate(body: &str, id: &str) -> Result<String> {
    let id = normalize_pending_id(id);
    let layout = PendingLayout::parse(body);
    let current = layout
        .items()
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| anyhow!("pending gate: no item with id [#{}]", id))?;
    let transition = validate_transition(current.state, PendingOp::Gate)?;
    let mut found = false;
    let mut changed = false;
    let rewritten = layout.replace_items(|item| {
        if item.id != id {
            return Some(item.clone());
        }
        found = true;
        match transition {
            TransitionResult::Transition(next) => {
                changed = true;
                let mut next_item = item.clone();
                next_item.state = next;
                Some(next_item)
            }
            TransitionResult::NoOp => Some(item.clone()),
        }
    });
    debug_assert!(found, "validated item must be present during replacement");
    if !changed {
        Ok(body.to_string())
    } else {
        Ok(rewritten.render())
    }
}

/// Transition an item to `Open` (`[ ]`) by id.
///
/// - `Gated → Open`: state mutates.
/// - `Open → *`: error (no source `[/]`).
/// - `Done → *`: error (cannot ungate a completed item).
pub fn op_ungate(body: &str, id: &str) -> Result<String> {
    let id = normalize_pending_id(id);
    let layout = PendingLayout::parse(body);
    let current = layout
        .items()
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| anyhow!("pending ungate: no item with id [#{}]", id))?;
    let transition = validate_transition(current.state, PendingOp::Ungate)?;
    let mut found = false;
    let mut changed = false;
    let rewritten = layout.replace_items(|item| {
        if item.id != id {
            return Some(item.clone());
        }
        found = true;
        match transition {
            TransitionResult::Transition(next) => {
                changed = true;
                let mut next_item = item.clone();
                next_item.state = next;
                next_item.gate_type = None;
                Some(next_item)
            }
            TransitionResult::NoOp => Some(item.clone()),
        }
    });
    debug_assert!(found, "validated item must be present during replacement");
    if !changed {
        Ok(body.to_string())
    } else {
        Ok(rewritten.render())
    }
}

/// Edit an item's text, preserving its hash id.
pub fn op_edit(body: &str, id: &str, new_text: &str) -> Result<String> {
    let new_text = new_text.trim();
    if new_text.is_empty() {
        bail!("pending edit: text must be non-empty");
    }
    let (parsed_text, parsed_continuation) = parse_pending_edit_payload(new_text)?;
    let id = normalize_pending_id(id);
    let mut found = false;
    let layout = PendingLayout::parse(body).replace_items(|item| {
        if item.id == id {
            found = true;
            let mut next = item.clone();
            next.text = parsed_text.clone();
            next.continuation = parsed_continuation.clone();
            Some(next)
        } else {
            Some(item.clone())
        }
    });
    if !found {
        return Err(anyhow!("pending edit: no item with id [#{}]", id));
    }
    Ok(layout.render())
}

/// Clear all items from the body. Non-item lines, including headers, are preserved.
pub fn op_clear(body: &str) -> Result<String> {
    let cleared = PendingLayout::parse(body).replace_items(|_| None);
    Ok(cleared.render())
}

/// Reorder items by id. Listed ids come first (in the given order); unlisted ids
/// keep their relative order and follow.
pub fn op_reorder(body: &str, ids: &[String]) -> Result<String> {
    let layout = PendingLayout::parse(body);
    let items = layout.items();
    let requested: Vec<String> = ids.iter().map(|s| normalize_pending_id(s)).collect();
    for id in &requested {
        if !items.iter().any(|i| i.id == *id) {
            bail!("pending reorder: no item with id [#{}]", id);
        }
    }
    let mut remaining: Vec<PendingItem> = items.clone();
    let mut ordered: Vec<PendingItem> = Vec::new();
    for id in &requested {
        if let Some(pos) = remaining.iter().position(|i| i.id == *id) {
            ordered.push(remaining.remove(pos));
        }
    }
    ordered.extend(remaining);
    let mut ordered_iter = ordered.into_iter();
    let reordered = layout.replace_items(|_| ordered_iter.next());
    Ok(reordered.render())
}

/// Resolve all gated items matching a typed gate. Finds items with `[/<gate_type>]`
/// and flips them to `[x]`. Returns `(new_body, resolved_ids)`.
///
/// Only matches typed gates — untyped `[/]` items are never resolved by this op.
pub fn op_resolve_gate(body: &str, gate_type: &str) -> (String, Vec<String>) {
    let gt = gate_type.trim().to_lowercase();
    let mut resolved = Vec::new();
    let layout = PendingLayout::parse(body).replace_items(|item| {
        if item.state == PendingState::Gated && item.gate_type.as_deref() == Some(gt.as_str()) {
            let mut next = item.clone();
            next.state = PendingState::Done;
            next.gate_type = None;
            resolved.push(next.id.clone());
            Some(next)
        } else {
            Some(item.clone())
        }
    });
    (layout.render(), resolved)
}

/// Set a typed gate on a gated item. The item must already be in `[/]` state.
/// Transitions `[/] → [/<gate_type>]`. Errors if the item is not gated.
pub fn op_set_gate_type(body: &str, id: &str, gate_type: &str) -> Result<String> {
    let id = normalize_pending_id(id);
    let gt = gate_type.trim().to_lowercase();
    if gt.is_empty()
        || !gt
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("invalid gate type: must be alphanumeric/dash/underscore");
    }
    let layout = PendingLayout::parse(body);
    let current = layout
        .items()
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| anyhow!("pending set-gate-type: no item with id [#{}]", id))?;
    if current.state != PendingState::Gated {
        bail!(
            "pending set-gate-type: item [#{}] must be gated ([/]) to set a typed gate, current state: [{}]",
            id,
            current.state.box_char()
        );
    }
    let mut found = false;
    let rewritten = layout.replace_items(|item| {
        if item.id != id {
            return Some(item.clone());
        }
        found = true;
        let mut next = item.clone();
        next.gate_type = Some(gt.clone());
        Some(next)
    });
    debug_assert!(found, "validated item must be present during replacement");
    Ok(rewritten.render())
}

/// Set (or replace) a typed proof/disproof verify predicate on a gated item
/// (`#optverify` / `#optv1`). The item must already be in `[/]` state. The
/// predicate is stored as an inline `<!-- gate-verify ... -->` annotation in the
/// item text (see [`crate::gate_verify`]); `set_at` stamps the gate-set time so
/// the later ops.log scan only counts markers emitted at/after it.
///
/// Errors if the item is missing, not gated, or the spec carries no matcher.
pub fn op_set_gate_verify(body: &str, id: &str, spec: &str, set_at: u64) -> Result<String> {
    let id = normalize_pending_id(id);
    let mut predicate = crate::gate_verify::parse_predicate_spec(spec);
    if !predicate.is_actionable() {
        bail!(
            "pending set-verify: spec must include verify=... and/or disproof=..., got: {}",
            spec
        );
    }
    predicate.set_at = Some(set_at);

    let layout = PendingLayout::parse(body);
    let current = layout
        .items()
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| anyhow!("pending set-verify: no item with id [#{}]", id))?;
    if current.state != PendingState::Gated {
        bail!(
            "pending set-verify: item [#{}] must be gated ([/]) to set a verify predicate, current state: [{}]",
            id,
            current.state.box_char()
        );
    }
    let mut found = false;
    let rewritten = layout.replace_items(|item| {
        if item.id != id {
            return Some(item.clone());
        }
        found = true;
        let mut next = item.clone();
        next.text = crate::gate_verify::upsert_annotation(&next.text, &predicate);
        Some(next)
    });
    debug_assert!(found, "validated item must be present during replacement");
    Ok(rewritten.render())
}

#[cfg(test)]
mod tests;
