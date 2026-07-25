//! # Module: backlog
//!
//! Pure functions for parsing and mutating the `agent:backlog` component body.
//!
//! Each pending item carries:
//! - a GFM task-list checkbox (`- [ ]` / `1. [ ]` or `- [x]` / `1. [x]`)
//! - an id prefix rendered as `[#xxxx]` (generated hash or caller-provided custom id)
//! - free-form text
//!
//! Canonical unordered form: `- [ ] [#a3f2] refactor preflight commit path`
//! Canonical ordered form: `1. [ ] [#a3f2] refactor preflight commit path`
//!
//! This module is I/O-free. Callers (`backlog_cmd.rs`, `preflight.rs`, `write.rs`)
//! handle reading/writing files, locking, and git commits.
//!
//! ## Spec
//! - Parser accepts legacy forms and normalizes via `backfill`.
//! - IDs are stable across edits/reorders; generated once (or supplied on insert), preserved thereafter.
//! - `reap` removes `- [x]` items. `detect_reorder` diffs id order between snapshot/current.
//! - Mutation ops (`op_add/done/edit/clear/reorder`) return a new body string, never mutate in place.

use agent_doc_element::element;
use anyhow::{Context, Result, anyhow, bail};
use std::collections::{BTreeMap, HashSet};

pub const IN_PROGRESS_MARKER: &str = "🚧";

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
    /// Visible, ephemeral marker for the item currently being worked from an
    /// active queue head. Renders immediately after the checkbox.
    pub in_progress: bool,
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
        let in_progress = if self.in_progress {
            format!(" {IN_PROGRESS_MARKER}")
        } else {
            String::new()
        };
        let mut out = format!(
            "{} {}{} [#{}] {}",
            self.marker.render_prefix(ordered_index),
            checkbox,
            in_progress,
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

pub fn format_shadow_refs(items: &[ShadowPendingItem]) -> String {
    items
        .iter()
        .map(ShadowPendingItem::reference)
        .collect::<Vec<_>>()
        .join(", ")
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MalformedTrackedItemRef {
    pub component: String,
    pub item: MalformedPendingItemLine,
}

impl MalformedTrackedItemRef {
    pub fn reference(&self) -> String {
        format!("{} {}", self.component, self.item.reference())
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

pub fn malformed_tracked_item_refs_in_components(
    content: &str,
    components: &[element::Component],
) -> Vec<MalformedTrackedItemRef> {
    components
        .iter()
        .filter(|component| element::is_tracked_work_component(&component.name))
        .flat_map(|component| {
            let component_name = component.name.clone();
            detect_malformed_item_lines(component.content(content))
                .into_iter()
                .map(move |item| MalformedTrackedItemRef {
                    component: component_name.clone(),
                    item,
                })
        })
        .collect()
}

pub fn malformed_tracked_item_refs(content: &str) -> Vec<MalformedTrackedItemRef> {
    let Ok(components) = element::parse(content) else {
        return Vec::new();
    };
    malformed_tracked_item_refs_in_components(content, &components)
}

pub fn malformed_tracked_item_interruption_message(refs: &[String]) -> String {
    format!(
        "[session-check] INTERRUPTED: malformed tracked checklist item(s) in live backlog/icebox: {}. Repair the checklist prefix before closeout so pending guards can prove the item state",
        refs.join("; ")
    )
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedComponentItemDrop {
    pub component: String,
    pub before: usize,
    pub after: usize,
}

/// Count tracked checklist items in every non-exchange component.
///
/// This is a document-level safety policy for operations that are supposed to
/// rewrite only `agent:exchange`. Component parse failures return no counts,
/// matching the historical fail-open comparison behavior in compact.
pub fn tracked_component_item_counts(content: &str) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    let Ok(components) = element::parse(content) else {
        return counts;
    };
    for component in &components {
        if component.name == "exchange" {
            continue;
        }
        let (_, items, _) = parse_items(component.content(content));
        if !items.is_empty() {
            counts.insert(component.name.clone(), items.len());
        }
    }
    counts
}

pub fn completed_items(body: &str) -> Vec<PendingItem> {
    let (_, items, _) = parse_items(body);
    items.into_iter().filter(PendingItem::is_done).collect()
}

pub fn completed_tracked_items_in_components(
    content: &str,
    components: &[element::Component],
) -> Vec<PendingItem> {
    components
        .iter()
        .filter(|component| element::is_tracked_work_component(&component.name))
        .flat_map(|component| completed_items(component.content(content)))
        .collect()
}

pub fn completed_tracked_items_in_content(content: &str) -> Result<Vec<PendingItem>> {
    let components = element::parse(content)?;
    Ok(completed_tracked_items_in_components(content, &components))
}

pub fn tracked_item_ref(item: &PendingItem) -> String {
    if item.id.is_empty() {
        format!("<missing-id> {}", item.text)
    } else {
        format!("#{}", item.id)
    }
}

pub fn tracked_item_refs(items: &[PendingItem]) -> Vec<String> {
    items.iter().map(tracked_item_ref).collect()
}

pub fn ensure_no_completed_tracked_items(content: &str, surface: &str) -> Result<()> {
    let completed = completed_tracked_items_in_content(content).with_context(|| {
        format!("failed to parse {surface} components during pending reap check")
    })?;
    if completed.is_empty() {
        return Ok(());
    }

    let refs = tracked_item_refs(&completed).join(", ");
    bail!("pending maintenance left completed tracked items in the {surface}: {refs}");
}

pub fn component_matches_tracked_surface(name: &str, surface: &str) -> bool {
    if element::is_backlog_component(surface) {
        element::is_backlog_component(name)
    } else {
        name == surface
    }
}

pub fn maintenance_surface_label(surface: &str) -> &'static str {
    if element::is_backlog_component(surface) {
        "pending"
    } else if element::is_review_component(surface) {
        "review"
    } else {
        "icebox"
    }
}

pub fn should_reap_already_done_mirrors(surface: &str) -> bool {
    element::is_backlog_component(surface) || element::is_review_component(surface)
}

pub fn should_reap_ops_proof_completions(surface: &str) -> bool {
    element::is_backlog_component(surface) || element::is_review_component(surface)
}

pub fn tracked_body_for_reorder(content: &str) -> Option<&str> {
    element::parse(content).ok().and_then(|comps| {
        comps
            .into_iter()
            .find(|component| element::is_backlog_component(&component.name))
            .map(|component| component.content(content))
    })
}

pub fn review_counts(content: &str) -> (usize, usize) {
    let Some(body) = element::parse(content).ok().and_then(|comps| {
        comps
            .into_iter()
            .find(|component| element::is_review_component(&component.name))
            .map(|component| component.content(content).to_string())
    }) else {
        return (0, 0);
    };
    let (_, items, _) = parse_items(&body);
    let review_items: Vec<_> = items.into_iter().filter(|item| !item.is_done()).collect();
    let gated = review_items
        .iter()
        .filter(|item| matches!(item.state, PendingState::Gated))
        .count();
    (review_items.len(), gated)
}

pub fn open_tracked_work_ids_in_content(content: &str) -> Vec<String> {
    let Ok(components) = element::parse(content) else {
        return Vec::new();
    };
    components
        .into_iter()
        .filter(|component| element::is_tracked_work_component(&component.name))
        .flat_map(|component| {
            let (_, items, _) = parse_items(component.content(content));
            items
        })
        .filter(|item| !item.is_done())
        .map(|item| item.id)
        .collect()
}

pub fn open_backlog_ids_in_content(content: &str) -> Vec<String> {
    let Ok(components) = element::parse(content) else {
        return Vec::new();
    };
    components
        .into_iter()
        .filter(|component| element::is_backlog_component(&component.name))
        .flat_map(|component| {
            let (_, items, _) = parse_items(component.content(content));
            items
        })
        .filter(|item| !item.is_done())
        .map(|item| item.id)
        .filter(|id| !id.is_empty())
        .collect()
}

pub fn tracked_work_ids_from_component_body(body: &str) -> HashSet<String> {
    let (_, items, _) = parse_items(body);
    items
        .into_iter()
        .filter(|item| !item.is_done())
        .map(|item| normalize_pending_id(&item.id))
        .filter(|id| !id.is_empty())
        .collect()
}

pub fn tracked_work_ids_for_target(
    content: &str,
    preferred_component: Option<&str>,
) -> Result<HashSet<String>> {
    let components = element::parse(content)?;
    let component = preferred_component
        .and_then(|name| components.iter().find(|component| component.name == name))
        .or_else(|| {
            components
                .iter()
                .find(|component| element::is_backlog_component(&component.name))
        })
        .or_else(|| {
            components
                .iter()
                .find(|component| element::is_tracked_work_component(&component.name))
        });
    Ok(component
        .map(|component| tracked_work_ids_from_component_body(component.content(content)))
        .unwrap_or_default())
}

/// Return non-exchange components whose tracked checklist item count decreased.
pub fn dropped_tracked_component_items(before: &str, after: &str) -> Vec<TrackedComponentItemDrop> {
    let before_counts = tracked_component_item_counts(before);
    let after_counts = tracked_component_item_counts(after);

    before_counts
        .into_iter()
        .filter_map(|(component, before)| {
            let after = after_counts.get(&component).copied().unwrap_or(0);
            (after < before).then_some(TrackedComponentItemDrop {
                component,
                before,
                after,
            })
        })
        .collect()
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

    let (in_progress, after_box) = consume_in_progress_marker(after_box);

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
        in_progress,
        text: text.trim_end().to_string(),
        continuation: String::new(),
    })
}

fn consume_in_progress_marker(text: &str) -> (bool, &str) {
    let mut rest = text.trim_start();
    let mut seen = false;
    while let Some(after_marker) = rest.strip_prefix(IN_PROGRESS_MARKER) {
        seen = true;
        rest = after_marker.trim_start();
    }
    (seen, rest)
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
pub enum TrackedWorkList {
    Backlog,
    Icebox,
}

impl TrackedWorkList {
    pub fn label(self) -> &'static str {
        match self {
            Self::Backlog => "backlog",
            Self::Icebox => "icebox",
        }
    }

    pub fn matches_component_name(self, name: &str) -> bool {
        match self {
            Self::Backlog => element::is_backlog_component(name),
            Self::Icebox => element::is_icebox_component(name),
        }
    }
}

pub fn find_tracked_work_component_in_content(
    content: &str,
    list: TrackedWorkList,
) -> Result<element::Component> {
    let components = element::parse(content).context("failed to parse components")?;
    components
        .into_iter()
        .find(|component| list.matches_component_name(&component.name))
        .with_context(|| format!("document has no {} component", list.label()))
}

/// Build the active document identity registry for tracked-work lookup.
///
/// Each normalized `#id` maps to the active sources that define it: frontmatter
/// `prompt_presets`, active backlog/review/icebox items, or both. Done items
/// and `agent:done` archives are intentionally excluded because they are not
/// active dispatch targets.
pub fn document_active_identities(content: &str) -> BTreeMap<String, Vec<String>> {
    let mut sources: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if let Ok((frontmatter, _)) = agent_doc_frontmatter::frontmatter::parse(content) {
        for key in frontmatter.prompt_presets.keys() {
            let id = normalize_pending_id(key);
            if !id.is_empty() {
                sources
                    .entry(id)
                    .or_default()
                    .push("prompt_presets".to_string());
            }
        }
    }
    if let Ok(components) = element::parse(content) {
        for component in components {
            let label = if element::is_backlog_component(&component.name) {
                "agent:backlog"
            } else if element::is_review_component(&component.name) {
                "agent:review"
            } else if element::is_icebox_component(&component.name) {
                "agent:icebox"
            } else {
                continue;
            };
            let (_, items, _) = parse_items(component.content(content));
            for item in items.into_iter().filter(|item| !item.is_done()) {
                let id = normalize_pending_id(&item.id);
                if !id.is_empty() {
                    sources.entry(id).or_default().push(label.to_string());
                }
            }
        }
    }
    sources
}

/// Collect identities that resolve under more than one active source.
///
/// When the same `#id` exists in two active sources, `do #id`, queue generation,
/// and "top backlog item: #id" are ambiguous between preset expansion and item
/// execution.
pub fn detect_identity_collisions(content: &str) -> Vec<String> {
    document_active_identities(content)
        .into_iter()
        .filter(|(_, srcs)| srcs.len() > 1)
        .map(|(id, srcs)| format!("#{id} ({})", srcs.join(" + ")))
        .collect()
}

/// Return existing active sources that a new explicit id would collide with.
pub fn identity_collision_for_new_id(content: &str, candidate_id: &str) -> Option<Vec<String>> {
    let candidate_id = normalize_pending_id(candidate_id);
    if candidate_id.is_empty() {
        return None;
    }
    document_active_identities(content)
        .get(&candidate_id)
        .filter(|sources| !sources.is_empty())
        .cloned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplicitIdCollision {
    pub candidate_id: String,
    pub sources: Vec<String>,
}

/// Return existing active sources that a new explicit item id would collide
/// with. Auto-id adds (no `id=<custom>` / `[#custom]` prefix) never collide.
pub fn explicit_new_item_id_collision(
    full_content: &str,
    item: &str,
) -> Option<ExplicitIdCollision> {
    let candidate_id = explicit_custom_id(item)?;
    let candidate_id = normalize_pending_id(&candidate_id);
    if candidate_id.is_empty() {
        return None;
    }
    let sources = identity_collision_for_new_id(full_content, &candidate_id)?;
    Some(ExplicitIdCollision {
        candidate_id,
        sources,
    })
}

/// Enforce that an explicit new tracked-work id has exactly one active meaning
/// in the document after insertion.
pub fn ensure_new_item_explicit_id_available(full_content: &str, item: &str) -> Result<()> {
    let Some(collision) = explicit_new_item_id_collision(full_content, item) else {
        return Ok(());
    };
    let candidate = collision.candidate_id;
    let sources = collision.sources.join(" + ");
    bail!(
        "pending add: refusing to add item with explicit id `#{candidate}` — that identity is already active under {sources}. Each #id must have exactly one active meaning per document so `do #id`, queue generation, and \"top backlog item\" stay unambiguous (#preset-item-id-collision-enforce). Choose a different id, or rename the existing {sources} entry first."
    );
}

pub fn find_open_tracked_work_component_in_content(
    content: &str,
    id: &str,
) -> Result<element::Component> {
    let id = normalize_pending_id(id);
    let components = element::parse(content).context("failed to parse components")?;
    components
        .into_iter()
        .find(|component| {
            if !element::is_tracked_work_component(&component.name) {
                return false;
            }
            let (_, items, _) = parse_items(component.content(content));
            items
                .into_iter()
                .any(|item| item.id == id && item.state != PendingState::Done)
        })
        .with_context(|| format!("id not found in backlog/icebox: {id}"))
}

pub fn open_tracked_work_component_name_in_content(
    content: &str,
    id: &str,
) -> Result<Option<String>> {
    let id = normalize_pending_id(id);
    let components = element::parse(content).context("failed to parse components")?;
    for component in components {
        if !element::is_tracked_work_component(&component.name) {
            continue;
        }
        let (_, items, _) = parse_items(component.content(content));
        if items
            .into_iter()
            .any(|item| item.id == id && item.state != PendingState::Done)
        {
            return Ok(Some(component.name));
        }
    }
    Ok(None)
}

pub fn content_has_resolved_tracked_work_id(content: &str, id: &str) -> Result<bool> {
    let id = normalize_pending_id(id);
    if id.is_empty() {
        return Ok(false);
    }

    let components = element::parse(content).context("failed to parse components")?;
    let archive_ref = format!("[#{id}]");
    for component in components {
        let body = component.content(content);
        if element::is_backlog_done_component(&component.name)
            && body
                .lines()
                .any(|line| line.to_ascii_lowercase().contains(&archive_ref))
        {
            return Ok(true);
        }
        if element::is_tracked_work_component(&component.name) {
            let (_, items, _) = parse_items(body);
            if items
                .into_iter()
                .any(|item| item.id == id && item.state == PendingState::Done)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn trim_tracked_parent_prefix(line: &str) -> &str {
    let trimmed = line.trim();
    if let Some(rest) = trimmed.strip_prefix("- ") {
        return rest.trim_start();
    }

    let digit_len = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digit_len == 0 {
        return trimmed;
    }
    let (_, tail) = trimmed.split_at(digit_len);
    let Some(tail) = tail.strip_prefix('.') else {
        return trimmed;
    };
    if !tail.starts_with(char::is_whitespace) {
        return trimmed;
    }
    tail.trim_start()
}

pub fn op_remove_matching_tracked_line(body: &str, target: &str, contains: bool) -> (String, bool) {
    let lines: Vec<&str> = body.lines().collect();
    let new_lines: Vec<String> = if contains {
        lines
            .iter()
            .filter(|line| !line.contains(target))
            .map(|line| line.to_string())
            .collect()
    } else {
        lines
            .iter()
            .filter(|line| trim_tracked_parent_prefix(line) != target)
            .map(|line| line.to_string())
            .collect()
    };
    let removed = new_lines.len() != lines.len();
    (new_lines.join("\n"), removed)
}

/// Non-empty tracked-work body lines as rendered by the CLI list command.
pub fn printable_tracked_work_lines(body: &str) -> Vec<String> {
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SymptomDedupeKey {
    pub invariant_id: String,
    pub document_id: String,
    pub component: String,
    pub content_hash: String,
}

impl SymptomDedupeKey {
    pub fn new(
        invariant_id: impl Into<String>,
        document_id: impl Into<String>,
        component: impl Into<String>,
        content_hash: impl Into<String>,
    ) -> Result<Self> {
        Ok(Self {
            invariant_id: validate_symptom_token("invariant", invariant_id.into())?,
            document_id: validate_symptom_token("document", document_id.into())?,
            component: validate_symptom_token("component", component.into())?,
            content_hash: validate_symptom_token("content_hash", content_hash.into())?,
        })
    }

    pub fn marker(&self) -> String {
        format!(
            "[symptom-key invariant={} document={} component={} content_hash={}]",
            self.invariant_id, self.document_id, self.component, self.content_hash
        )
    }

    pub fn log_fields(&self) -> String {
        format!(
            "invariant={} document={} component={} content_hash={}",
            self.invariant_id, self.document_id, self.component, self.content_hash
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAddOutcome {
    pub body: String,
    pub id: String,
    pub inserted: bool,
    pub deduped_key: Option<SymptomDedupeKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAddBatchItemOutcome {
    pub id: String,
    pub inserted: bool,
    pub deduped_key: Option<SymptomDedupeKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAddBatchOutcome {
    pub body: String,
    pub outcomes: Vec<PendingAddBatchItemOutcome>,
}

pub fn symptom_dedupe_key_from_text(text: &str) -> Result<Option<SymptomDedupeKey>> {
    let Some(marker_start) = text.find("[symptom-key ") else {
        return Ok(None);
    };
    let marker_body_start = marker_start + "[symptom-key ".len();
    let Some(marker_end_rel) = text[marker_body_start..].find(']') else {
        bail!("symptom-key marker is missing closing `]`");
    };
    let marker_body = &text[marker_body_start..marker_body_start + marker_end_rel];

    let mut invariant_id = None;
    let mut document_id = None;
    let mut component = None;
    let mut content_hash = None;
    for field in marker_body.split_whitespace() {
        let Some((name, value)) = field.split_once('=') else {
            bail!("symptom-key field must be key=value: {field}");
        };
        let slot = match name {
            "invariant" => &mut invariant_id,
            "document" => &mut document_id,
            "component" => &mut component,
            "content_hash" => &mut content_hash,
            _ => bail!("unknown symptom-key field `{name}`"),
        };
        if slot.is_some() {
            bail!("duplicate symptom-key field `{name}`");
        }
        *slot = Some(validate_symptom_token(name, value.to_string())?);
    }

    Ok(Some(SymptomDedupeKey::new(
        invariant_id.context("symptom-key missing invariant")?,
        document_id.context("symptom-key missing document")?,
        component.context("symptom-key missing component")?,
        content_hash.context("symptom-key missing content_hash")?,
    )?))
}

fn validate_symptom_token(field: &str, value: String) -> Result<String> {
    if value.trim().is_empty() {
        bail!("symptom-key {field} must not be empty");
    }
    if value.trim() != value || !value.chars().all(is_symptom_token_char) {
        bail!("symptom-key {field} must be a single field-safe token: {value}");
    }
    Ok(value)
}

fn is_symptom_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/')
}

fn strip_symptom_dedupe_key_marker(text: &str) -> String {
    let Some(marker_start) = text.find("[symptom-key ") else {
        return text.trim().to_string();
    };
    let marker_body_start = marker_start + "[symptom-key ".len();
    let Some(marker_end_rel) = text[marker_body_start..].find(']') else {
        return text.trim().to_string();
    };
    let marker_end = marker_body_start + marker_end_rel + 1;
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..marker_start]);
    out.push_str(&text[marker_end..]);
    collapse_inline_spaces(&out)
}

fn symptom_evidence_line(text: &str) -> String {
    let evidence = strip_symptom_dedupe_key_marker(text);
    if evidence.is_empty() {
        "  evidence: repeated symptom".to_string()
    } else {
        format!("  evidence: {evidence}")
    }
}

fn append_unique_continuation_line(continuation: &str, line: &str) -> String {
    if continuation
        .lines()
        .any(|existing| existing.trim() == line.trim())
    {
        return continuation.to_string();
    }
    let mut out = continuation.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(line);
    out.push('\n');
    out
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

/// Promote a leading bare `#tag` to the item id. A bare leading tag is treated
/// as the caller's intended id when it is a clean pending id (ASCII alphanumeric
/// plus hyphen) immediately followed by whitespace and more item text. Mid-text
/// `#tag` references are never promoted — only a tag at the very front of the
/// line qualifies, and a trailing non-whitespace char (e.g. `=` in
/// `#lazilyspecpin=Pin …`) disqualifies it so compound topic labels keep their
/// auto hash id. Returns `(id, remaining_text)` or `None`.
///
/// This closes the "agent-doc ignores the explicit id" gap: an agent that adds
/// `#mergestatemachine3 JB+VSCode report…` now gets `[#mergestatemachine3] …`
/// instead of a generated hash with the tag left dangling in the text.
fn leading_bare_tag_id(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim_start();
    let after_hash = trimmed.strip_prefix('#')?;
    let end = after_hash
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or(after_hash.len());
    if end == 0 {
        return None;
    }
    let token = &after_hash[..end];
    if !is_valid_pending_id(token) {
        return None;
    }
    // The token must be terminated by whitespace (not punctuation like `=`), and
    // there must be remaining item text — a bare `#tag` that IS the whole line is
    // a reference, not an id request, and is left untouched.
    let separator = after_hash[end..].chars().next()?;
    if !separator.is_whitespace() {
        return None;
    }
    let rest = after_hash[end..].trim_start();
    if rest.is_empty() {
        return None;
    }
    Some((token.to_lowercase(), rest.to_string()))
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
        Ok((id, _)) => {
            if id.is_some() {
                id
            } else {
                // No `id=` / `[#id]` prefix; a leading bare `#tag` is also an
                // explicit id request (promoted at add time), so collision
                // enforcement must see it too.
                leading_bare_tag_id(item).map(|(id, _)| id)
            }
        }
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
    let components = agent_doc_element::element::parse(doc)?;
    let Some(backlog_component) = components
        .iter()
        .find(|component| agent_doc_element::element::is_backlog_component(&component.name))
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
            agent_doc_element::element::is_tracked_work_component(&component.name)
                || component.name == "exchange"
                // `agent:queue` `[#id]` / `do [#id]` heads are legitimate queue
                // references to backlog items, NOT shadow backlog items hiding in
                // commented-out prose. A reaped id's lingering queue head (e.g.
                // `[#jbacceptwedge]: 0.2.170 installed`) or any `#mirrorall`-mirrored
                // head must not be misclassified as an "open backlog item outside
                // the live backlog" and hard-wedge preflight — the done-strike
                // maintenance pass owns clearing resolved queue heads
                // (#qheadsync orphan-shadow). The shadow guard targets stray prose
                // (HTML comments, non-component sections), so exclude the queue too.
                || component.name == "queue"
        })
        .map(|component| (component.open_start, component.close_end))
        .collect();
    let code_ranges = agent_doc_element::element::find_code_ranges(doc);

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

pub fn format_dropped_refs(items: &[DroppedBacklogItem]) -> String {
    items
        .iter()
        .map(DroppedBacklogItem::reference)
        .collect::<Vec<_>>()
        .join(", ")
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
    let baseline_components = agent_doc_element::element::parse(baseline_doc)?;
    let Some(baseline_backlog) = baseline_components
        .iter()
        .find(|c| agent_doc_element::element::is_backlog_component(&c.name))
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

    let current_components = agent_doc_element::element::parse(current_doc)?;

    let mut current_ids: HashSet<String> = HashSet::new();

    for comp in &current_components {
        if agent_doc_element::element::is_tracked_work_component(&comp.name) {
            let (_, items, _) = parse_items(comp.content(current_doc));
            for item in items {
                if !item.id.is_empty() {
                    current_ids.insert(item.id);
                }
            }
        } else if agent_doc_element::element::is_backlog_done_component(&comp.name) {
            current_ids.extend(extract_pending_ids_from_text(comp.content(current_doc)));
        }
    }
    current_ids.extend(extra_current_ids.iter().cloned());

    let excluded_ranges: Vec<(usize, usize)> = current_components
        .iter()
        .filter(|c| agent_doc_element::element::is_tracked_work_component(&c.name))
        .map(|c| (c.open_start, c.close_end))
        .collect();
    let code_ranges = agent_doc_element::element::find_code_ranges(current_doc);

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
    let today = today_civil_day();
    PendingLayout::parse(body)
        .items()
        .into_iter()
        .filter(|item| matches!(item.state, PendingState::Open) && !item.id.is_empty())
        .filter(|item| !item_precondition_unmet(&item.text, today))
        .map(|item| item.id.clone())
        .collect()
}

/// Set the ephemeral in-progress marker on exactly the requested tracked item ids,
/// clearing stale markers from every other parsed item in the body.
pub fn set_in_progress_item_ids(body: &str, ids: &HashSet<String>) -> (String, bool) {
    let normalized_ids: HashSet<String> = ids
        .iter()
        .map(|id| normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();
    let mut changed = false;
    let rewritten = PendingLayout::parse(body).replace_items(|item| {
        let mut next = item.clone();
        let should_mark = !next.id.is_empty()
            && !matches!(next.state, PendingState::Done)
            && normalized_ids.contains(&next.id.to_ascii_lowercase());
        if next.in_progress != should_mark {
            next.in_progress = should_mark;
            changed = true;
        }
        Some(next)
    });
    let new_body = rewritten.render();
    if new_body != body {
        changed = true;
    }
    (new_body, changed)
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
    let today = today_civil_day();
    PendingLayout::parse(body)
        .items()
        .into_iter()
        .filter(|item| matches!(item.state, PendingState::Open) && !item.id.is_empty())
        .filter(|item| item_has_enqueue_marker(&item.text))
        .filter(|item| !item_precondition_unmet(&item.text, today))
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
    let today = today_civil_day();
    PendingLayout::parse(body)
        .items()
        .into_iter()
        .filter(|item| matches!(item.state, PendingState::Open) && !item.id.is_empty())
        .filter(|item| !item_precondition_unmet(&item.text, today))
        .map(|item| (item.id.clone(), item_priority_rank(&item.text)))
        .collect()
}

/// Parse a `not-before=YYYY-MM-DD` scheduling precondition from an item's text
/// (`#backlog-not-before`). Returns the threshold as a day number (days since
/// 1970-01-01) so callers can compare it against [`today_civil_day`]. The token
/// must start at a word boundary so prose does not match, and only a strict
/// zero-or-more-digit `YYYY-MM-DD` is accepted; a malformed value is ignored (the
/// item is then treated as having no precondition rather than silently held).
pub fn item_not_before_day(text: &str) -> Option<i64> {
    const TOK: &str = "not-before=";
    let mut search = 0;
    while let Some(rel) = text[search..].find(TOK) {
        let idx = search + rel;
        let boundary_ok = idx == 0
            || !text[..idx]
                .chars()
                .next_back()
                .map(|c| c.is_ascii_alphanumeric() || c == '-')
                .unwrap_or(false);
        search = idx + TOK.len();
        if !boundary_ok {
            continue;
        }
        let value: String = text[search..]
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-')
            .collect();
        if let Some(day) = parse_ymd_to_civil_day(&value) {
            return Some(day);
        }
    }
    None
}

fn parse_ymd_to_civil_day(value: &str) -> Option<i64> {
    let parts: Vec<&str> = value.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let y: i64 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    let d: u32 = parts[2].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

/// Days from 1970-01-01 for a proleptic Gregorian calendar date (Howard
/// Hinnant's `days_from_civil`). Negative before the epoch.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = ((m + 9) % 12) as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Today as a day number (days since 1970-01-01, UTC) from the system clock,
/// for evaluating `not-before=` scheduling preconditions. Best-effort: a clock
/// before the epoch clamps to day 0.
pub fn today_civil_day() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86_400) as i64)
        .unwrap_or(0)
}

/// True when an item's `not-before=` scheduling precondition is still in the
/// future relative to `today` (so it must be held out of the backlog→queue
/// sync). An item with no `not-before=` token, or with a threshold on/<= `today`,
/// is eligible (`#backlog-not-before`).
pub fn item_precondition_unmet(text: &str, today: i64) -> bool {
    item_not_before_day(text).is_some_and(|day| day > today)
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

/// Author-controlled execution-context tag literals on a backlog item
/// (`#goqueuestall`). They live as bracketed tokens in the item text alongside
/// markers like `[recommended]` / `[TOP]`, and gate whether a `go`-mode queue
/// may auto-drain the item in the current session type.
pub const CLEAN_SESSION_TAG: &str = "[clean-session]";
pub const OPERATOR_VERIFY_TAG: &str = "[operator-verify]";
pub const FOCUSED_CYCLE_TAG: &str = "[focused-cycle]";

/// Machine-readable execution-context attributes parsed from a backlog item's
/// text (`#goqueuestall`).
///
/// - `clean_session_required` — `[clean-session]`: the item must run from a
///   session WITHOUT a live editor-IPC listener; running it under a live IPC
///   listener risks closeout corruption, so go-mode skips it while live. NOTE
///   (`#qcontdrain`): `[clean-session]` items now DRAIN IN PLACE in the
///   in-session loop, so this flag no longer defers the continuation decision.
/// - `operator_verify_required` — `[operator-verify]`: completion needs
///   operator-driven live verification the agent cannot perform, so go-mode
///   never auto-drains it.
/// - `focused_cycle_required` — `[focused-cycle]` (`#qstallguard` Layer A): the
///   OPERATOR has declared that this item needs its own dedicated, operator-
///   initiated cycle (e.g. merge-core / supervisor-core work that needs
///   `make tmux-ci` across live panes), so it must NOT be auto-drained inside
///   the queue loop even though the agent could perform the work. This is the
///   ONLY sanctioned way to mark an agent-doable item non-loop-drainable: it
///   moves the "this needs a focused cycle" judgment OUT of the agent's prose
///   reading (where it became a stall excuse) and INTO a binary-read tag. The
///   agent must never re-derive non-drainability from an item's description —
///   absent this tag, a drainable head is drained.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExecutionContext {
    pub clean_session_required: bool,
    pub operator_verify_required: bool,
    pub focused_cycle_required: bool,
}

impl ExecutionContext {
    /// True when at least one deferral tag is present.
    pub fn is_deferred(&self) -> bool {
        self.clean_session_required || self.operator_verify_required || self.focused_cycle_required
    }

    /// True when the item is undrainable by the IN-SESSION queue loop — it needs a
    /// human (`[operator-verify]`) or a dedicated, freshly-cleared cycle
    /// (`[focused-cycle]`). `[clean-session]` is intentionally EXCLUDED: it
    /// drains in place (`#qcontdrain`). This is the single authority for
    /// "the in-session loop must not auto-drain this head" (`#qstallguard` Layer A).
    pub fn loop_undrainable(&self) -> bool {
        self.operator_verify_required || self.focused_cycle_required
    }

    /// True when the item is undrainable even by the SUPERVISOR clear-and-continue
    /// drain — it needs a human (`[operator-verify]`). Unlike
    /// [`Self::loop_undrainable`], `[focused-cycle]` is NOT supervisor-undrainable
    /// (`#qfocsup`): the supervisor idle-watch force-`/clear`s the session and
    /// re-dispatches a `[focused-cycle]` head to a genuinely fresh context, which
    /// is exactly the fresh cycle the tag demands. So a `[focused-cycle]` item is
    /// deferred by the in-session loop (which cannot give it fresh context) but
    /// DRAINED by the supervisor's clear-and-continue path instead of stranding the
    /// queue idle. `[clean-session]` drains everywhere.
    pub fn supervisor_undrainable(&self) -> bool {
        self.operator_verify_required
    }
}

/// True when the item text carries a `[clean-session]` tag token
/// (`#goqueuestall`). Matched case-insensitively as a whitespace-delimited
/// bracketed token so prose mentions of the word do not trip it.
pub fn item_clean_session_required(text: &str) -> bool {
    text_has_bracket_tag(text, "clean-session")
}

/// True when the item text carries an `[operator-verify]` tag token
/// (`#goqueuestall`).
pub fn item_operator_verify_required(text: &str) -> bool {
    text_has_bracket_tag(text, "operator-verify")
}

/// True when the item text carries a `[focused-cycle]` tag token
/// (`#qstallguard` Layer A).
pub fn item_focused_cycle_required(text: &str) -> bool {
    text_has_bracket_tag(text, "focused-cycle")
}

/// Parse all execution-context tags from an item's text (`#goqueuestall` /
/// `#qstallguard`).
pub fn item_execution_context(text: &str) -> ExecutionContext {
    ExecutionContext {
        clean_session_required: item_clean_session_required(text),
        operator_verify_required: item_operator_verify_required(text),
        focused_cycle_required: item_focused_cycle_required(text),
    }
}

fn text_has_bracket_tag(text: &str, tag: &str) -> bool {
    text.split_whitespace().any(|token| {
        let trimmed = token.trim_matches(|c: char| matches!(c, '(' | ')' | ',' | '.' | ';' | ':'));
        trimmed.len() == tag.len() + 2
            && trimmed.starts_with('[')
            && trimmed.ends_with(']')
            && trimmed[1..trimmed.len() - 1].eq_ignore_ascii_case(tag)
    })
}

/// Remove the `[clean-session]` / `[operator-verify]` execution-context tags
/// from display text (`#goqueuestall`), collapsing the whitespace they leave
/// behind. Mirrors how machine-only markers are kept out of rendered prose.
pub fn strip_execution_context_tags(text: &str) -> String {
    let kept: Vec<&str> = text
        .split_whitespace()
        .filter(|token| {
            let trimmed =
                token.trim_matches(|c: char| matches!(c, '(' | ')' | ',' | '.' | ';' | ':'));
            !(trimmed.eq_ignore_ascii_case(CLEAN_SESSION_TAG)
                || trimmed.eq_ignore_ascii_case(OPERATOR_VERIFY_TAG)
                || trimmed.eq_ignore_ascii_case(FOCUSED_CYCLE_TAG))
        })
        .collect();
    kept.join(" ")
}

/// Active (open) backlog item ids paired with their parsed execution context,
/// in document order (`#goqueuestall`). Feeds the go-mode backlog→queue sync
/// skip and the queue-continuation drainable-head computation.
pub fn active_item_execution_contexts(body: &str) -> Vec<(String, ExecutionContext)> {
    PendingLayout::parse(body)
        .items()
        .into_iter()
        .filter(|item| matches!(item.state, PendingState::Open) && !item.id.is_empty())
        .map(|item| (item.id.clone(), item_execution_context(&item.text)))
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

/// Ordered unique bare `#id` references in free text.
///
/// Unlike [`extract_pending_ids_from_text`], this accepts both bracketed
/// `[#id]` and bare `#id` forms and preserves first-seen order. It is used by
/// prompt/directive parsers that need deterministic target order.
pub fn extract_pending_hash_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let chars = text.char_indices().collect::<Vec<_>>();
    let mut idx = 0usize;

    while idx < chars.len() {
        let (byte_idx, ch) = chars[idx];
        if ch != '#' {
            idx += 1;
            continue;
        }

        let start = byte_idx + ch.len_utf8();
        let mut end = start;
        let mut cursor = idx + 1;
        while cursor < chars.len() {
            let (next_byte, next_ch) = chars[cursor];
            if next_ch.is_ascii_alphanumeric() || next_ch == '-' || next_ch == '_' {
                end = next_byte + next_ch.len_utf8();
                cursor += 1;
                continue;
            }
            break;
        }

        if end > start {
            let id = normalize_pending_id(&text[start..end]);
            if !ids.iter().any(|existing| existing == &id) {
                ids.push(id);
            }
        }
        idx = cursor.max(idx + 1);
    }

    ids
}

/// Extract only each done/archive **item's own** `#id` — the FIRST `[#id]` on a
/// list-item line (`- <date> [#id] …` / `- [x] [#id] …`) — and ignore any other
/// `[#id]` cited in that item's prose.
///
/// `#donemirrorreap`: the whole-text [`extract_pending_ids_from_text`] harvests
/// every bracketed id anywhere in the text, so a `[#other]` cited inside one done
/// entry's body (e.g. "behind do `[#fullboundary]`" inside the `#ftstrike` entry)
/// is wrongly treated as done — which then falsely reaps a still-open `#other`
/// review/backlog mirror. An item's identity is its leading id, never a citation
/// in its description, so done-id collection must use this per-item extractor.
pub fn extract_done_item_own_ids(text: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        // Only list-item lines carry an item's own id; prose / continuation lines
        // contribute none (a cited id there is a reference, not an identity).
        if !(trimmed.starts_with("- ") || trimmed.starts_with("* ")) {
            continue;
        }
        // The item's own id is the FIRST bracketed id on the line; a leading
        // checkbox (`[x]` / `[ ]` / `[/]`) has no `#` so `find("[#")` skips it.
        if let Some(start) = trimmed.find("[#") {
            let after = &trimmed[start + 2..];
            if let Some(end) = after.find(']') {
                let id = &after[..end];
                if is_valid_pending_id(id) {
                    ids.insert(id.to_ascii_lowercase());
                }
            }
        }
    }
    ids
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

/// Maximum length of a derived representative id, before any collision suffix.
const REPRESENTATIVE_ID_MAX_LEN: usize = 24;

/// How many representative keywords are joined into a derived id.
const REPRESENTATIVE_ID_WORDS: usize = 3;

/// Shortest keyword worth carrying into a representative id.
const REPRESENTATIVE_ID_MIN_WORD_LEN: usize = 3;

/// Structural noise and low-signal English that never distinguishes one tracked
/// item from another. `agent`/`doc` are included because nearly every item in an
/// agent-doc session document mentions them, so they carry no selectivity.
const REPRESENTATIVE_ID_STOPWORDS: &[&str] = &[
    "the", "and", "but", "for", "not", "are", "was", "were", "been", "being", "this", "that",
    "these", "those", "should", "would", "could", "will", "shall", "can", "may", "must", "does",
    "did", "done", "have", "has", "had", "with", "from", "into", "out", "over", "under", "again",
    "all", "any", "some", "more", "most", "than", "then", "when", "where", "which", "what", "why",
    "how", "its", "it", "you", "our", "we", "use", "used", "using", "make", "makes", "made",
    "please", "also", "still", "just", "only", "even", "new", "get", "gets", "set", "sets", "add",
    "adds", "via", "per", "each", "both", "there", "their", "they", "them", "here", "about",
    "instead", "rather", "because", "since", "while", "after", "before", "agent", "doc",
    "agentdoc",
];

/// Strip fenced blocks, inline code spans, and blockquoted context so a derived
/// id keys off the operator's own directive rather than pasted logs or a quoted
/// prior response.
fn representative_id_source(text: &str) -> String {
    let mut source = String::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") || trimmed.starts_with('>') {
            break;
        }
        source.push_str(line);
        source.push('\n');
    }
    // Drop inline code spans: `do [#task-id]` contributes "task"/"id", which are
    // structural, not representative.
    let mut cleaned = String::with_capacity(source.len());
    let mut in_code = false;
    for ch in source.chars() {
        if ch == '`' {
            in_code = !in_code;
            cleaned.push(' ');
            continue;
        }
        if !in_code {
            cleaned.push(ch);
        }
    }
    // Drop `#tag` references. A tag in an item's text names OTHER work; minting
    // it as this item's identity would invent an id the operator never assigned
    // and collide with the thing being referenced.
    cleaned
        .split_whitespace()
        .filter(|word| !word.trim_start_matches(['[', '(']).starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Derive a human-readable id from an item's own words, or `None` when the text
/// carries no distinctive keywords (an empty, emoji-only, or pure-boilerplate
/// item) and a surrogate hash is the honest answer.
///
/// Keywords are taken in document order rather than by frequency: the operator's
/// opening clause is the part that names the work, and order-preserving output
/// stays predictable and reproducible. Repeats are dropped so a text that leans
/// on one term still yields a distinguishing id.
pub fn derive_representative_id(text: &str) -> Option<String> {
    let source = representative_id_source(text);
    let mut words: Vec<String> = Vec::new();
    for raw in source
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        let word = raw.to_ascii_lowercase();
        if word.len() < REPRESENTATIVE_ID_MIN_WORD_LEN
            || word.chars().all(|c| c.is_ascii_digit())
            || REPRESENTATIVE_ID_STOPWORDS.contains(&word.as_str())
            || words.contains(&word)
        {
            continue;
        }
        words.push(word);
        if words.len() == REPRESENTATIVE_ID_WORDS {
            break;
        }
    }
    if words.is_empty() {
        return None;
    }
    let mut id = words.concat();
    if id.len() > REPRESENTATIVE_ID_MAX_LEN {
        id.truncate(REPRESENTATIVE_ID_MAX_LEN);
    }
    is_valid_pending_id(&id).then_some(id)
}

/// Assign an id that does not collide with `taken`.
///
/// Prefers a representative id derived from the item's own words
/// (`#freetextqueue` over `#0dsr`) so a queue head, commit message, or code
/// comment referencing it resolves to something a reader can place. Falls back
/// to the surrogate hash when the text yields no distinctive keywords, and
/// appends a short hash to a representative id that is already taken so
/// readability never costs uniqueness.
fn assign_unique_id(text: &str, doc_id: &str, taken: &HashSet<String>) -> String {
    if let Some(representative) = derive_representative_id(text) {
        if !taken.contains(&representative) {
            return representative;
        }
        for counter in 0..16u64 {
            let suffix = generate_hash_n(text, doc_id, counter, 4);
            let candidate = format!("{representative}-{suffix}");
            if !taken.contains(&candidate) {
                return candidate;
            }
        }
    }
    assign_unique_hash(text, doc_id, taken)
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

/// Render a tracked-work component body in canonical form, assigning any
/// missing ids against an otherwise empty active-id registry.
pub fn canonicalize_tracked_work_body(body: &str, doc_id: &str) -> String {
    let (canonical, _) = backfill(body, doc_id, &HashSet::new());
    canonical
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

pub fn op_add_with_outcome(
    body: &str,
    text: &str,
    doc_id: &str,
    gated: bool,
) -> Result<PendingAddOutcome> {
    op_add_at_with_outcome(body, text, doc_id, gated, AddPosition::First)
}

/// Prepend multiple new tracked-work items while preserving caller order.
///
/// Each single add prepends to the front, so the batch is applied in reverse and
/// item outcomes are returned in the caller's original order.
pub fn op_prepend_many_with_outcomes(
    body: &str,
    items: &[String],
    doc_id: &str,
    gated: bool,
) -> Result<PendingAddBatchOutcome> {
    let mut body = body.to_string();
    let mut outcomes = Vec::with_capacity(items.len());
    for item in items.iter().rev() {
        let outcome = op_add_with_outcome(&body, item, doc_id, gated)?;
        body = outcome.body;
        outcomes.push(PendingAddBatchItemOutcome {
            id: outcome.id,
            inserted: outcome.inserted,
            deduped_key: outcome.deduped_key,
        });
    }
    outcomes.reverse();
    Ok(PendingAddBatchOutcome { body, outcomes })
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
    let outcome = op_add_at_with_outcome(body, text, doc_id, gated, position)?;
    Ok((outcome.body, outcome.id))
}

/// Add a pending item and report whether it inserted a new item or attached
/// evidence to an existing symptom with the same de-duplication key.
pub fn op_add_at_with_outcome(
    body: &str,
    text: &str,
    doc_id: &str,
    gated: bool,
    position: AddPosition<'_>,
) -> Result<PendingAddOutcome> {
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
    if custom_id.is_none() {
        if let Some((tag_id, cleaned)) = leading_bare_tag_id(&text) {
            inline_custom_id = Some(tag_id);
            text = cleaned;
        } else if let Some((tag_id, cleaned)) = extract_inline_tag_id(&text) {
            inline_custom_id = Some(tag_id);
            text = cleaned;
        }
    }

    let mut layout = PendingLayout::parse(body);
    let items = layout.items();

    if let Some(key) = symptom_dedupe_key_from_text(&text)?
        && let Some(existing) = items
            .iter()
            .find(|item| {
                item.state != PendingState::Done
                    && symptom_dedupe_key_from_text(&item.text)
                        .ok()
                        .flatten()
                        .as_ref()
                        == Some(&key)
            })
            .cloned()
    {
        let evidence = symptom_evidence_line(&text);
        let rewritten = layout.replace_items(|item| {
            if item.id == existing.id {
                let mut next = item.clone();
                next.continuation = append_unique_continuation_line(&next.continuation, &evidence);
                Some(next)
            } else {
                Some(item.clone())
            }
        });
        return Ok(PendingAddOutcome {
            body: rewritten.render(),
            id: existing.id,
            inserted: false,
            deduped_key: Some(key),
        });
    }

    // Dedup: adding the same active item is idempotent. This matters for
    // closeout repair: a prior attempt may have captured the backlog item but
    // failed before committing the response, and the retry should satisfy the
    // capture guard without creating a duplicate row.
    if let Some(existing) = items
        .iter()
        .find(|i| i.state != PendingState::Done && i.text == text)
    {
        return Ok(PendingAddOutcome {
            body: body.to_string(),
            id: existing.id.clone(),
            inserted: false,
            deduped_key: None,
        });
    }
    if items.iter().any(|i| i.text == text) {
        bail!(
            "pending add: duplicate completed item text already exists: {}",
            text
        );
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
        assign_unique_id(&text, doc_id, &taken)
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
        in_progress: false,
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
    Ok(PendingAddOutcome {
        body: layout.render(),
        id,
        inserted: true,
        deduped_key: None,
    })
}

/// Mark an item `[x]` by id. Phase 1: state-machine validation lives in
/// the upcoming `backlog_cmd` layer; this primitive forces Done unconditionally.
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

/// Apply multiple text edits in order to one tracked-work body.
pub fn op_edit_many(body: &str, edits: &[(String, String)]) -> Result<String> {
    let mut next = body.to_string();
    for (id, text) in edits {
        next = op_edit(&next, id, text)?;
    }
    Ok(next)
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
mod tests {
    use super::*;

    const DOC_ID: &str = "test-doc";

    fn ids() -> HashSet<String> {
        HashSet::new()
    }

    const TRACKED_COMPONENT_DOC: &str = concat!(
        "---\nagent_doc_session: drop-test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: topic one\n\nResponse one.\n",
        "<!-- /agent:exchange -->\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#a1] item one\n",
        "- [ ] [#a2] item two\n",
        "- [ ] [#a3] item three\n",
        "<!-- /agent:backlog -->\n\n",
        "## Review\n\n",
        "<!-- agent:review -->\n",
        "- [/] [#r1] review one\n",
        "<!-- /agent:review -->\n",
    );

    #[test]
    fn tracked_component_item_counts_counts_backlog_and_review() {
        let counts = tracked_component_item_counts(TRACKED_COMPONENT_DOC);
        assert_eq!(counts.get("backlog").copied(), Some(3));
        assert_eq!(counts.get("review").copied(), Some(1));
        assert_eq!(counts.get("exchange"), None);
    }

    #[test]
    fn completed_tracked_items_are_projected_from_tracked_components() {
        let doc = concat!(
            "<!-- agent:exchange -->\n",
            "- [x] [#not-tracked] response list\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#open] open item\n",
            "- [x] [#done] done item\n",
            "<!-- /agent:backlog -->\n",
            "<!-- agent:review -->\n",
            "- [X] [#review-done] review done item\n",
            "<!-- /agent:review -->\n",
        );
        let completed = completed_tracked_items_in_content(doc).unwrap();
        assert_eq!(
            completed
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["done", "review-done"]
        );
        assert_eq!(
            tracked_item_refs(&completed),
            vec!["#done".to_string(), "#review-done".to_string()]
        );
        assert!(ensure_no_completed_tracked_items(TRACKED_COMPONENT_DOC, "working tree").is_ok());
        let err = ensure_no_completed_tracked_items(doc, "working tree")
            .expect_err("completed tracked items should fail the guard")
            .to_string();
        assert!(err.contains("working tree"));
        assert!(err.contains("#done"));
        assert!(err.contains("#review-done"));
    }

    #[test]
    fn tracked_surface_policy_handles_pending_alias_review_and_icebox() {
        assert!(component_matches_tracked_surface("backlog", "pending"));
        assert!(component_matches_tracked_surface("pending", "backlog"));
        assert!(component_matches_tracked_surface("review", "review"));
        assert!(!component_matches_tracked_surface("icebox", "review"));

        assert_eq!(maintenance_surface_label("backlog"), "pending");
        assert_eq!(maintenance_surface_label("review"), "review");
        assert_eq!(maintenance_surface_label("icebox"), "icebox");

        assert!(should_reap_already_done_mirrors("backlog"));
        assert!(should_reap_already_done_mirrors("review"));
        assert!(!should_reap_already_done_mirrors("icebox"));

        assert!(should_reap_ops_proof_completions("pending"));
        assert!(should_reap_ops_proof_completions("review"));
        assert!(!should_reap_ops_proof_completions("icebox"));
    }

    #[test]
    fn tracked_surface_body_and_review_counts_are_document_projections() {
        assert_eq!(
            tracked_body_for_reorder(TRACKED_COMPONENT_DOC).map(str::trim),
            Some("- [ ] [#a1] item one\n- [ ] [#a2] item two\n- [ ] [#a3] item three")
        );
        assert_eq!(review_counts(TRACKED_COMPONENT_DOC), (1, 1));

        let no_review = "<!-- agent:backlog -->\n- [ ] [#a] item\n<!-- /agent:backlog -->\n";
        assert_eq!(review_counts(no_review), (0, 0));
    }

    #[test]
    fn open_tracked_work_ids_in_content_lists_non_done_tracked_items() {
        let doc = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: ignored\n\nbody\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#open] open backlog\n",
            "- [/] [#gated] gated backlog\n",
            "- [x] [#done] done backlog\n",
            "<!-- /agent:backlog -->\n",
            "<!-- agent:review -->\n",
            "- [ ] [#review] open review\n",
            "<!-- /agent:review -->\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#ice] open icebox\n",
            "<!-- /agent:icebox -->\n",
        );

        assert_eq!(
            open_tracked_work_ids_in_content(doc),
            vec![
                "open".to_string(),
                "gated".to_string(),
                "review".to_string(),
                "ice".to_string(),
            ]
        );
    }

    #[test]
    fn open_backlog_ids_in_content_lists_only_non_done_backlog_items() {
        let doc = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#open] open backlog\n",
            "- [/] [#gated] gated backlog\n",
            "- [x] [#done] done backlog\n",
            "<!-- /agent:backlog -->\n",
            "<!-- agent:review -->\n",
            "- [ ] [#review] open review\n",
            "<!-- /agent:review -->\n",
        );

        assert_eq!(
            open_backlog_ids_in_content(doc),
            vec!["open".to_string(), "gated".to_string()]
        );
    }

    #[test]
    fn tracked_work_ids_from_component_body_lists_normalized_non_done_items() {
        let ids = tracked_work_ids_from_component_body(concat!(
            "- [ ] [#OPEN] open item\n",
            "- [/] [#Gated] gated item\n",
            "- [x] [#done] done item\n",
        ));

        assert_eq!(
            ids,
            ["open", "gated"].into_iter().map(str::to_string).collect()
        );
    }

    #[test]
    fn tracked_work_ids_for_target_prefers_requested_component_then_backlog() {
        let doc = concat!(
            "<!-- agent:review -->\n",
            "- [ ] [#review] review item\n",
            "<!-- /agent:review -->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#backlog] backlog item\n",
            "<!-- /agent:backlog -->\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#ice] icebox item\n",
            "<!-- /agent:icebox -->\n",
        );

        assert_eq!(
            tracked_work_ids_for_target(doc, Some("review")).unwrap(),
            ["review"].into_iter().map(str::to_string).collect()
        );
        assert_eq!(
            tracked_work_ids_for_target(doc, None).unwrap(),
            ["backlog"].into_iter().map(str::to_string).collect()
        );
    }

    #[test]
    fn dropped_tracked_component_items_is_empty_when_only_exchange_changes() {
        let after = TRACKED_COMPONENT_DOC.replace("Response one.", "*Compacted.*");
        assert!(dropped_tracked_component_items(TRACKED_COMPONENT_DOC, &after).is_empty());
    }

    #[test]
    fn dropped_tracked_component_items_reports_decreased_component_count() {
        let after = TRACKED_COMPONENT_DOC.replace("- [ ] [#a2] item two\n", "");
        assert_eq!(
            dropped_tracked_component_items(TRACKED_COMPONENT_DOC, &after),
            vec![TrackedComponentItemDrop {
                component: "backlog".to_string(),
                before: 3,
                after: 2,
            }]
        );
    }

    #[test]
    fn extract_pending_hash_ids_preserves_order_and_accepts_bare_ids() {
        let ids = extract_pending_hash_ids("do [#alpha] then #beta then #alpha and #under_score");
        assert_eq!(ids, vec!["alpha", "beta", "under_score"]);
    }

    #[test]
    fn extract_done_item_own_ids_ignores_prose_citations() {
        // #donemirrorreap regression: the #ftstrike done entry cites [#fullboundary]
        // in its prose. The own-id extractor must return only ftstrike, never
        // fullboundary (which would falsely reap an open #fullboundary mirror).
        let archive = concat!(
            "# Agent Doc Completed Work\n\n",
            "- 2026-06-19 [#733r] reconcile queue heads.\n",
            "- 2026-06-19 [#ftstrike] strike free-text heads regardless of position, ",
            "e.g. a head behind do [#fullboundary] is now struck. See [#semmerge].\n",
        );
        let got = extract_done_item_own_ids(archive);
        assert!(got.contains("733r"));
        assert!(got.contains("ftstrike"));
        assert!(
            !got.contains("fullboundary"),
            "a [#id] cited in prose must NOT be treated as a done item id: {got:?}"
        );
        assert!(
            !got.contains("semmerge"),
            "trailing prose citation excluded: {got:?}"
        );
        // Contrast: the whole-text extractor DOES harvest the prose citations.
        assert!(extract_pending_ids_from_text(archive).contains("fullboundary"));
    }

    #[test]
    fn extract_done_item_own_ids_handles_checkbox_and_skips_prose_lines() {
        let body = concat!(
            "- [x] [#foo] done thing depends on [#bar].\n",
            "  continuation prose mentioning [#baz]\n",
            "- [/] [#gated] gated item\n",
            "plain prose line with [#qux]\n",
        );
        let got = extract_done_item_own_ids(body);
        assert!(got.contains("foo"));
        assert!(got.contains("gated"));
        assert!(
            !got.contains("bar"),
            "same-line prose citation excluded: {got:?}"
        );
        assert!(
            !got.contains("baz"),
            "continuation-line citation excluded: {got:?}"
        );
        assert!(
            !got.contains("qux"),
            "non-list prose line excluded: {got:?}"
        );
    }

    #[test]
    fn active_item_ids_returns_open_items_in_order() {
        let body = concat!(
            "- [ ] [#first] one\n",
            "- [/] [#gated] blocked\n",
            "- [x] [#done] finished\n",
            "- [ ] [#second] two\n",
        );
        assert_eq!(active_item_ids(body), vec!["first", "second"]);
    }

    #[test]
    fn in_progress_marker_after_checkbox_preserves_pending_id() {
        let body = "- [ ] 🚧 [#first] one\n- [/] 🚧 [#gated] blocked\n";
        let (_, items, _) = parse_items(body);

        assert_eq!(items[0].id, "first");
        assert!(items[0].in_progress);
        assert_eq!(items[1].id, "gated");
        assert!(items[1].in_progress);
        assert_eq!(active_item_ids(body), vec!["first"]);
        assert_eq!(render_items("", &items, ""), body);
    }

    #[test]
    fn set_in_progress_item_ids_moves_marker_and_clears_stale_items() {
        let body = "- [ ] 🚧 [#old] old\n- [ ] [#new] new\n";
        let ids = ["new".to_string()].into_iter().collect();

        let (updated, changed) = set_in_progress_item_ids(body, &ids);

        assert!(changed);
        assert_eq!(updated, "- [ ] [#old] old\n- [ ] 🚧 [#new] new\n");
    }

    #[test]
    fn set_in_progress_item_ids_never_marks_done_items() {
        let body = concat!(
            "- [x] 🚧 [#done] finished\n",
            "- [x] [#target] already finished\n",
            "- [/] [#review] active review\n",
        );
        let ids = [
            "done".to_string(),
            "target".to_string(),
            "review".to_string(),
        ]
        .into_iter()
        .collect();

        let (updated, changed) = set_in_progress_item_ids(body, &ids);

        assert!(changed);
        assert_eq!(
            updated,
            concat!(
                "- [x] [#done] finished\n",
                "- [x] [#target] already finished\n",
                "- [/] 🚧 [#review] active review\n",
            )
        );
    }

    #[test]
    fn active_item_ids_empty_for_empty_body() {
        assert!(active_item_ids("").is_empty());
    }

    #[test]
    fn active_item_ids_holds_future_not_before_items() {
        // #backlog-not-before: a `not-before=` precondition in the future holds
        // the item out of the backlog→queue sync; a past threshold is eligible.
        // 2020-01-01 is always past and 2999-12-31 always future for any real
        // test-run clock, so no clock injection is needed.
        let body = concat!(
            "- [ ] [#now] ready now\n",
            "- [ ] [#past] not-before=2020-01-01 already due\n",
            "- [ ] [#future] not-before=2999-12-31 scheduled later\n",
        );
        assert_eq!(active_item_ids(body), vec!["now", "past"]);
        assert_eq!(
            active_item_priorities(body)
                .into_iter()
                .map(|(id, _)| id)
                .collect::<Vec<_>>(),
            vec!["now", "past"]
        );
    }

    #[test]
    fn active_enqueue_item_ids_holds_future_not_before_items() {
        // A future `not-before=` precondition overrides an explicit enqueue marker.
        let body = concat!(
            "- [ ] [#mark] /enqueue not-before=2999-12-31 scheduled later\n",
            "- [ ] [#ready] /enqueue not-before=2020-01-01 due\n",
        );
        assert_eq!(active_enqueue_item_ids(body), vec!["ready"]);
    }

    #[test]
    fn item_not_before_day_parses_and_validates() {
        assert_eq!(item_not_before_day("not-before=1970-01-01 x"), Some(0));
        assert_eq!(item_not_before_day("text not-before=1970-01-02 x"), Some(1));
        // word boundary: prose / hyphenated prefix must not match
        assert_eq!(item_not_before_day("xnot-before=2020-01-01"), None);
        // malformed → ignored (treated as no precondition)
        assert_eq!(item_not_before_day("not-before=2020-13-01"), None);
        assert_eq!(item_not_before_day("not-before=garbage"), None);
        assert_eq!(item_not_before_day("no token here"), None);
    }

    #[test]
    fn item_precondition_unmet_compares_against_today() {
        let today = days_from_civil(2026, 6, 18);
        assert!(item_precondition_unmet("not-before=2026-06-19 x", today));
        assert!(!item_precondition_unmet("not-before=2026-06-18 x", today)); // due today
        assert!(!item_precondition_unmet("not-before=2026-06-17 x", today));
        assert!(!item_precondition_unmet("no precondition", today));
    }

    #[test]
    fn active_enqueue_item_ids_returns_marked_open_items() {
        let body = concat!(
            "- [ ] [#inbox] :inbox_tray: one\n",
            "- [/] [#gated] :inbox_tray: blocked\n",
            "- [x] [#done] **enqueue** finished\n",
            "- [ ] [#bold] **enqueue** two\n",
            "- [ ] [#slash] /enqueue three\n",
            "- [ ] [#plain] enqueue should not be a marker\n",
            "- [ ] untracked :inbox_tray: no id\n",
        );
        assert_eq!(
            active_enqueue_item_ids(body),
            vec!["inbox", "bold", "slash"]
        );
    }

    #[test]
    fn execution_context_tags_parse_booleans() {
        // #goqueuestall: `[clean-session]` / `[operator-verify]` flip booleans.
        let clean = item_execution_context("[clean-session] needs a quiet session");
        assert!(clean.clean_session_required);
        assert!(!clean.operator_verify_required);
        assert!(clean.is_deferred());

        let oper = item_execution_context("ship it [operator-verify] live drive");
        assert!(!oper.clean_session_required);
        assert!(oper.operator_verify_required);
        assert!(oper.is_deferred());

        // Plain items set neither and are not deferred.
        let plain = item_execution_context("just do the thing");
        assert!(!plain.clean_session_required);
        assert!(!plain.operator_verify_required);
        assert!(!plain.is_deferred());

        // Both tags combine, and they coexist with `[recommended]` / `[TOP]`.
        let both =
            item_execution_context("[recommended] [TOP] [clean-session] [operator-verify] go");
        assert!(both.clean_session_required);
        assert!(both.operator_verify_required);

        // Prose mention of the word (not a bracketed token) does not trip it.
        let prose = item_execution_context("run from a clean session please");
        assert!(!prose.clean_session_required);
    }

    #[test]
    fn focused_cycle_tag_is_loop_undrainable_but_clean_session_is_not() {
        // `#qstallguard` Layer A: `[focused-cycle]` is the operator's binary-read
        // knob for "agent-doable but needs its own dedicated cycle, do not
        // auto-drain in the loop."
        let focused = item_execution_context("[focused-cycle] supervisor-core merge change");
        assert!(focused.focused_cycle_required);
        assert!(focused.is_deferred());
        assert!(
            focused.loop_undrainable(),
            "[focused-cycle] must be undrainable by the queue loop"
        );

        // `[clean-session]` is deferred-flavored but DRAINS IN PLACE — it must NOT
        // be loop_undrainable (`#qcontdrain`).
        let clean = item_execution_context("[clean-session] needs a quiet session");
        assert!(clean.is_deferred());
        assert!(
            !clean.loop_undrainable(),
            "[clean-session] drains in place — never loop_undrainable"
        );

        // `[operator-verify]` needs a human and is loop_undrainable.
        let oper = item_execution_context("[operator-verify] live drive");
        assert!(oper.loop_undrainable());

        // `#qfocsup`: `[focused-cycle]` is loop_undrainable but SUPERVISOR-drainable
        // — the supervisor force-`/clear`s and re-dispatches it to a fresh context,
        // so it must NOT be supervisor_undrainable. Only `[operator-verify]` is.
        assert!(
            !focused.supervisor_undrainable(),
            "[focused-cycle] is drained by the supervisor clear-and-continue path"
        );
        assert!(
            oper.supervisor_undrainable(),
            "[operator-verify] needs a human — undrainable by any agent scope"
        );
        assert!(
            !clean.supervisor_undrainable(),
            "[clean-session] drains everywhere"
        );

        // Plain items are fully drainable: the agent may NOT invent a stall.
        let plain = item_execution_context("just implement #6b5h");
        assert!(!plain.loop_undrainable());
        assert!(!plain.supervisor_undrainable());
        assert!(!plain.is_deferred());

        // The tag strips out of display text and coexists with other markers.
        assert_eq!(
            strip_execution_context_tags("[recommended] [focused-cycle] rework the merge seam"),
            "[recommended] rework the merge seam"
        );
    }

    #[test]
    fn execution_context_tags_stripped_from_display_text() {
        // #goqueuestall: tags are kept out of rendered display text.
        assert_eq!(
            strip_execution_context_tags("[clean-session] fix the converge path"),
            "fix the converge path"
        );
        assert_eq!(
            strip_execution_context_tags("[recommended] [operator-verify] live verify the flow"),
            "[recommended] live verify the flow"
        );
        assert_eq!(
            strip_execution_context_tags("[clean-session] [operator-verify] both"),
            "both"
        );
        // No tags → unchanged words (whitespace normalized on rejoin).
        assert_eq!(
            strip_execution_context_tags("plain item text"),
            "plain item text"
        );
    }

    #[test]
    fn active_item_execution_contexts_skips_closed_and_idless() {
        // #goqueuestall: only open, id-bearing items surface a context.
        let body = concat!(
            "- [ ] [#a] [clean-session] needs quiet\n",
            "- [x] [#b] [operator-verify] already done\n",
            "- [ ] [#c] plain drainable\n",
        );
        let ctxs = active_item_execution_contexts(body);
        assert_eq!(ctxs.len(), 2);
        assert_eq!(ctxs[0].0, "a");
        assert!(ctxs[0].1.clean_session_required);
        assert_eq!(ctxs[1].0, "c");
        assert!(!ctxs[1].1.is_deferred());
    }

    #[test]
    fn item_priority_rank_parses_token() {
        assert_eq!(item_priority_rank("priority=1 do the thing"), 1);
        assert_eq!(item_priority_rank("text priority=5 more"), 5);
        assert_eq!(item_priority_rank("no token here"), PRIORITY_RANK_UNSET);
        assert_eq!(
            item_priority_rank("priority=0 out of range"),
            PRIORITY_RANK_UNSET
        );
        assert_eq!(
            item_priority_rank("priority=12 out of range"),
            PRIORITY_RANK_UNSET
        );
    }

    #[test]
    fn sort_by_priority_orders_ascending_stable() {
        let body = concat!(
            "- [ ] [#a] priority=3 third\n",
            "- [ ] [#b] no priority\n",
            "- [ ] [#c] priority=1 first\n",
            "- [ ] [#d] priority=1 also first\n",
        );
        let sorted = sort_by_priority(body).expect("order should change");
        // priority=1 items first (stable: c before d), then priority=3, then unset.
        let ids: Vec<&str> = sorted
            .lines()
            .filter_map(|l| l.split("[#").nth(1))
            .filter_map(|s| s.split(']').next())
            .collect();
        assert_eq!(ids, vec!["c", "d", "a", "b"]);
    }

    #[test]
    fn sort_by_priority_idempotent_when_ordered() {
        let body = "- [ ] [#a] priority=1 x\n- [ ] [#b] priority=2 y\n";
        assert!(sort_by_priority(body).is_none());
    }

    #[test]
    fn active_item_priorities_pairs_open_ids_with_rank() {
        let body = concat!(
            "- [ ] [#a] priority=2 one\n",
            "- [/] [#g] priority=1 gated\n",
            "- [ ] [#b] two\n",
        );
        assert_eq!(
            active_item_priorities(body),
            vec![
                ("a".to_string(), 2u8),
                ("b".to_string(), PRIORITY_RANK_UNSET)
            ]
        );
    }

    #[test]
    fn op_take_active_items_by_ids_removes_open_and_gated_matches_only() {
        let body = concat!(
            "intro\n",
            "- [ ] [#open] remove open\n",
            "  child line\n",
            "- [/] [#gated] remove gated\n",
            "- [x] [#done] keep explicit done\n",
            "- [ ] [#keep] keep open\n",
        );
        let ids: HashSet<String> = ["#open".to_string(), "gated".to_string(), "done".to_string()]
            .into_iter()
            .collect();

        let (new_body, removed) = op_take_active_items_by_ids(body, &ids);

        assert_eq!(
            removed
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["open", "gated"]
        );
        assert!(!new_body.contains("[#open]"));
        assert!(!new_body.contains("child line"));
        assert!(!new_body.contains("[#gated]"));
        assert!(new_body.contains("[#done] keep explicit done"));
        assert!(new_body.contains("[#keep] keep open"));
        assert!(new_body.starts_with("intro\n"));
    }

    #[test]
    fn parse_empty_body() {
        let (p, items, post) = parse_items("");
        assert_eq!(p, "");
        assert!(items.is_empty());
        assert_eq!(post, "");
    }

    #[test]
    fn parse_fully_migrated() {
        let body = "- [ ] [#a3f2] first\n- [x] [#b1c4] second\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].marker, PendingListMarker::Bullet);
        assert_eq!(items[0].id, "a3f2");
        assert_eq!(items[0].state, PendingState::Open);
        assert_eq!(items[0].text, "first");
        assert_eq!(items[1].id, "b1c4");
        assert_eq!(items[1].state, PendingState::Done);
    }

    #[test]
    fn detects_malformed_tracked_checklist_lines() {
        let body = concat!(
            "_- [ ] [#pcops] damaged prefix\n",
            "- [ ] [#keep1] valid item\n",
            "plain note mentioning [#note1]\n",
        );

        let malformed = detect_malformed_item_lines(body);

        assert_eq!(malformed.len(), 1);
        assert_eq!(malformed[0].id, "pcops");
        assert_eq!(malformed[0].line, 1);
        assert!(malformed[0].text.contains("damaged prefix"));
    }

    #[test]
    fn malformed_tracked_item_refs_scan_tracked_components() {
        let doc = concat!(
            "<!-- agent:backlog -->\n",
            "_- [ ] [#backlog1] damaged backlog\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n",
            "_- [ ] [#review1] damaged review\n",
            "<!-- /agent:review -->\n\n",
            "<!-- agent:exchange -->\n",
            "_- [ ] [#exchange1] ignored exchange\n",
            "<!-- /agent:exchange -->\n",
        );

        let refs = malformed_tracked_item_refs(doc);

        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].component, "backlog");
        assert_eq!(refs[0].item.id, "backlog1");
        assert_eq!(
            refs[0].reference(),
            "backlog #backlog1 (line 1): _- [ ] [#backlog1] damaged backlog"
        );
        assert_eq!(refs[1].component, "review");
        assert_eq!(refs[1].item.id, "review1");
    }

    #[test]
    fn malformed_tracked_item_interruption_message_formats_refs() {
        let message = malformed_tracked_item_interruption_message(&[
            "backlog #a1 (line 1): _- [ ] [#a1] damaged".to_string(),
        ]);

        assert!(message.contains("malformed tracked checklist item"));
        assert!(message.contains("backlog #a1 (line 1)"));
        assert!(message.contains("pending guards can prove the item state"));
    }

    #[test]
    fn parse_hyphenated_id() {
        let body = "- [ ] [#tmuxcrash-abcd] child task\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "tmuxcrash-abcd");
        assert_eq!(items[0].text, "child task");
    }

    #[test]
    fn parse_ordered_items() {
        let body = "1. [ ] [#a3f2] first\n2. [x] [#b1c4] second\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].marker, PendingListMarker::Ordered(1));
        assert_eq!(items[0].id, "a3f2");
        assert_eq!(items[1].marker, PendingListMarker::Ordered(2));
        assert_eq!(items[1].state, PendingState::Done);
    }

    #[test]
    fn strips_redundant_self_id_tag_from_text() {
        // #pending-redundant-self-id-strip: a self-id repeated after a tag is dropped.
        let body = "- [ ] [#stale-x] [recommended] [#stale-x] Evaluate the retire path.\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "stale-x");
        assert_eq!(items[0].text, "[recommended] Evaluate the retire path.");
    }

    #[test]
    fn preserves_cross_reference_ids_in_text() {
        // Only the item's OWN id is stripped; references to other ids stay.
        let body = "- [ ] [#a] depends on [#b] and [#c] downstream.\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "a");
        assert_eq!(items[0].text, "depends on [#b] and [#c] downstream.");
    }

    #[test]
    fn parse_gated_state() {
        let body = "- [/] [#eg0w] CommitLock — gate: v0.32.5\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].state, PendingState::Gated);
        assert_eq!(items[0].id, "eg0w");
        assert_eq!(items[0].text, "CommitLock — gate: v0.32.5");
    }

    #[test]
    fn parse_all_three_states() {
        let body = "- [ ] [#a3f2] open\n- [/] [#b1c4] gated\n- [x] [#c9e0] done\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].state, PendingState::Open);
        assert_eq!(items[1].state, PendingState::Gated);
        assert_eq!(items[2].state, PendingState::Done);
    }

    #[test]
    fn parse_legacy_tilde_checkbox_preserves_hash_id() {
        let body = "- [~] [#q6js] in-progress legacy item\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].state, PendingState::Open);
        assert_eq!(items[0].id, "q6js");
        assert_eq!(items[0].text, "in-progress legacy item");
    }

    #[test]
    fn parse_checkbox_only_no_id() {
        let body = "- [ ] just text\n- [x] done item\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "");
        assert_eq!(items[0].text, "just text");
        assert_eq!(items[1].state, PendingState::Done);
    }

    #[test]
    fn parse_legacy_no_checkbox() {
        let body = "- legacy one\n- legacy two\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "legacy one");
        assert_eq!(items[0].state, PendingState::Open);
        assert_eq!(items[0].id, "");
    }

    #[test]
    fn parse_mixed() {
        let body = "- [ ] [#a3f2] migrated\n- [ ] partial\n- legacy\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].id, "a3f2");
        assert_eq!(items[1].id, "");
        assert_eq!(items[1].text, "partial");
        assert_eq!(items[2].text, "legacy");
    }

    #[test]
    fn parse_nested_lines_attach_to_parent_item() {
        let body = concat!(
            "- [ ] [#a3f2] parent task\n",
            "  - dependency one\n",
            "  - dependency two\n",
            "- [ ] [#b1c4] sibling task\n"
        );
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "a3f2");
        assert_eq!(items[0].text, "parent task");
        assert_eq!(
            items[0].continuation,
            "  - dependency one\n  - dependency two\n"
        );
        assert_eq!(items[1].id, "b1c4");
        assert!(items[1].continuation.is_empty());
    }

    #[test]
    fn parse_nested_lines_attach_to_ordered_parent_item() {
        let body = concat!(
            "1. [ ] [#a3f2] parent task\n",
            "   1. dependency one\n",
            "   2. dependency two\n",
            "2. [ ] [#b1c4] sibling task\n"
        );
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].marker, PendingListMarker::Ordered(1));
        assert_eq!(
            items[0].continuation,
            "   1. dependency one\n   2. dependency two\n"
        );
        assert_eq!(items[1].marker, PendingListMarker::Ordered(2));
    }

    #[test]
    fn render_roundtrip_canonical() {
        let body = "- [ ] [#a3f2] first\n- [x] [#b1c4] second\n";
        let (p, items, post) = parse_items(body);
        let out = render_items(&p, &items, &post);
        assert_eq!(out, body);
    }

    #[test]
    fn render_roundtrip_all_three_states() {
        let body = "- [ ] [#a3f2] open\n- [/] [#b1c4] gated — gate: v0.32.5\n- [x] [#c9e0] done\n";
        let (p, items, post) = parse_items(body);
        let out = render_items(&p, &items, &post);
        assert_eq!(out, body);
    }

    #[test]
    fn render_roundtrip_ordered_list() {
        let body = "1. [ ] [#a3f2] open\n2. [/] [#b1c4] gated\n3. [x] [#c9e0] done\n";
        let (p, items, post) = parse_items(body);
        let out = render_items(&p, &items, &post);
        assert_eq!(out, body);
    }

    #[test]
    fn render_emits_slash_for_gated() {
        let item = PendingItem {
            marker: PendingListMarker::Bullet,
            id: "eg0w".to_string(),
            state: PendingState::Gated,
            gate_type: None,
            in_progress: false,
            text: "CommitLock".to_string(),
            continuation: String::new(),
        };
        assert_eq!(item.render(), "- [/] [#eg0w] CommitLock");
    }

    #[test]
    fn backfill_adds_hashes() {
        let body = "- legacy one\n- legacy two\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items.len(), 2);
        assert!(!items[0].id.is_empty());
        assert!(!items[1].id.is_empty());
        assert_ne!(items[0].id, items[1].id);
        assert!(new_body.contains("- [ ] [#"));
    }

    #[test]
    fn backfill_idempotent() {
        let body = "- [ ] [#a3f2] first\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(!changed, "fully-migrated body should not change");
        assert_eq!(new_body, body);
    }

    #[test]
    fn backfill_normalizes_checkbox_only() {
        let body = "- [ ] no id here\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert!(new_body.contains("[#"));
    }

    #[test]
    fn backfill_drops_content_less_empty_bullet() {
        // #icebox-empty-item-phantom-id: a stray empty `- [ ]` must NOT be
        // assigned a phantom hash id; it carries no description and is dropped.
        let body = "- [ ]\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert!(
            !new_body.contains("[#"),
            "empty bullet must not get a phantom id: {new_body:?}"
        );
        let (_, items, _) = parse_items(&new_body);
        assert!(
            items.is_empty(),
            "empty bullet should be dropped: {items:?}"
        );
    }

    #[test]
    fn backfill_drops_id_only_empty_item_self_heal() {
        // Already-cemented phantom (`- [ ] [#1k5y]` with no description) is
        // removed on the next backfill so the bug self-heals.
        let body = "- [ ] [#existing] real item\n- [ ] [#1k5y]\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert!(new_body.contains("[#existing] real item"));
        assert!(
            !new_body.contains("[#1k5y]"),
            "phantom id-only item must be dropped: {new_body:?}"
        );
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn backfill_keeps_empty_text_with_continuation() {
        // Guard against over-dropping: an item with empty header text but a real
        // indented continuation still carries content and must be preserved.
        let body = "- [ ] [#p1]\n  - detail line\n";
        let (new_body, _changed) = backfill(body, DOC_ID, &ids());
        assert!(
            new_body.contains("[#p1]"),
            "item with continuation must survive: {new_body:?}"
        );
        assert!(new_body.contains("detail line"));
    }

    #[test]
    fn backfill_never_inserts_gated() {
        // Legacy items with no checkbox must default to Open `[ ]`,
        // never Gated `[/]`. Gated state is always operator-explicit.
        let body = "- legacy item awaiting v0.32.5\n";
        let (new_body, _) = backfill(body, DOC_ID, &ids());
        assert!(new_body.contains("- [ ] "));
        assert!(!new_body.contains("- [/] "));
    }

    #[test]
    fn backfill_preserves_existing_gated() {
        let body = "- [/] [#eg0w] CommitLock — gate: v0.32.5\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(!changed);
        assert_eq!(new_body, body);
    }

    #[test]
    fn backfill_preserves_id_behind_legacy_tilde_checkbox() {
        let body = "- [~] [#q6js] pcpc5e3 08b cut-over proof\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert_eq!(new_body, "- [ ] [#q6js] pcpc5e3 08b cut-over proof\n");
        assert_eq!(new_body.matches("[#").count(), 1);

        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].state, PendingState::Open);
        assert_eq!(items[0].id, "q6js");
        assert_eq!(items[0].text, "pcpc5e3 08b cut-over proof");
    }

    #[test]
    fn backfill_preserves_interleaved_headers_and_blank_lines() {
        let body = concat!(
            "### Active\n",
            "- legacy one\n",
            "\n",
            "### Later\n",
            "- [ ] [#keep1] keep section\n"
        );
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert!(new_body.contains("### Active"));
        assert!(new_body.contains("\n\n### Later\n"));
        assert!(new_body.contains("[#keep1] keep section"));
        let lines: Vec<&str> = new_body.lines().collect();
        assert_eq!(lines[0], "### Active");
        assert!(lines[1].starts_with("- [ ] [#"));
        assert_eq!(lines[3], "### Later");
    }

    #[test]
    fn backfill_assigns_nested_subtask_ids_prefixed_by_parent() {
        let body = concat!(
            "- parent task\n",
            "  - child dependency\n",
            "  - child subtask\n",
            "- sibling task\n"
        );
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert_eq!(new_body.matches("[#").count(), 4, "got: {new_body}");
        assert!(new_body.contains("  - [ ] [#"));
        let lines: Vec<&str> = new_body.lines().collect();
        let parent_line = lines[0];
        let parent_id = parent_line
            .split("[#")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .expect("parent id");
        assert!(
            lines[1].contains(&format!("[#{}-", parent_id)),
            "expected first child id prefixed by parent id, got: {}",
            lines[1]
        );
        assert!(
            lines[2].contains(&format!("[#{}-", parent_id)),
            "expected second child id prefixed by parent id, got: {}",
            lines[2]
        );
    }

    #[test]
    fn backfill_preserves_existing_nested_subtask_ids() {
        let body = concat!(
            "- [ ] [#tmuxcrash] parent task\n",
            "  - [ ] [#tmuxcrash-abcd] child dependency\n"
        );
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(!changed);
        assert_eq!(new_body, body);
    }

    #[test]
    fn backfill_reassigns_duplicate_existing_nested_subtask_ids() {
        let body = concat!(
            "- [ ] [#tmuxcrash] parent task\n",
            "  - [ ] [#tmuxcrash-abcd] child dependency\n",
            "  - [ ] [#tmuxcrash-abcd] child subtask\n"
        );
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        let lines: Vec<&str> = new_body.lines().collect();
        assert_eq!(lines.len(), 3, "got: {new_body}");
        assert!(lines[1].contains("[#tmuxcrash-abcd]"));
        let second_child_id = lines[2]
            .split("[#")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .expect("second child id");
        assert_ne!(second_child_id, "tmuxcrash-abcd");
        assert!(second_child_id.starts_with("tmuxcrash-"));
    }

    #[test]
    fn backfill_preserves_ordered_parent_items() {
        let body = concat!("1. legacy one\n", "2. [ ] [#keep1] keep section\n");
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert!(new_body.starts_with("1. [ ] [#"));
        assert!(new_body.contains("2. [ ] [#keep1] keep section"));
    }

    #[test]
    fn reap_skips_gated() {
        let body = "- [/] [#eg0w] gated\n- [x] [#c9e0] done\n";
        let (new_body, removed) = reap(body).unwrap();
        assert_eq!(removed, vec!["c9e0"]);
        assert!(new_body.contains("[#eg0w]"));
        assert!(!new_body.contains("[#c9e0]"));
    }

    #[test]
    fn reap_removes_checked() {
        let body = "- [ ] [#a3f2] keep\n- [x] [#b1c4] drop\n- [ ] [#c5d6] keep2\n";
        let (new_body, removed) = reap(body).unwrap();
        assert_eq!(removed, vec!["b1c4"]);
        assert!(new_body.contains("a3f2"));
        assert!(!new_body.contains("b1c4"));
        assert!(new_body.contains("c5d6"));
    }

    #[test]
    fn reap_removes_flush_left_spill_with_completed_parent() {
        let body = concat!(
            "- [x] [#b1c4] drop\n",
            "Commands:\n",
            "  cargo test -p agent-doc backlog::\n",
            "Diff:\n",
            "@@ -1 +1 @@\n",
            "- [ ] [#c5d6] keep\n"
        );
        let (new_body, removed) = reap(body).unwrap();
        assert_eq!(removed, vec!["b1c4"]);
        assert!(!new_body.contains("[#b1c4]"));
        assert!(!new_body.contains("Commands:"));
        assert!(!new_body.contains("@@ -1 +1 @@"));
        assert!(new_body.contains("- [ ] [#c5d6] keep"));
    }

    #[test]
    fn reap_preserves_following_heading_text() {
        let body = concat!(
            "- [x] [#b1c4] drop\n",
            "\n",
            "## Next Group\n",
            "- [ ] [#c5d6] keep\n"
        );
        let (new_body, removed) = reap(body).unwrap();
        assert_eq!(removed, vec!["b1c4"]);
        assert!(!new_body.contains("[#b1c4]"));
        assert!(new_body.contains("## Next Group"));
        assert!(new_body.contains("- [ ] [#c5d6] keep"));
    }

    #[test]
    fn reap_noop_when_none_checked() {
        let body = "- [ ] [#a3f2] keep\n";
        let (new_body, removed) = reap(body).unwrap();
        assert!(removed.is_empty());
        assert_eq!(new_body, body);
    }

    #[test]
    fn reap_errors_when_completed_item_is_missing_id() {
        let body = "- [x] legacy done without id\n- [ ] [#keep1] keep\n";
        let err = reap(body).unwrap_err();
        assert!(
            err.to_string()
                .contains("pending reap requires ids for completed items"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn reap_with_items_archives_malformed_flush_left_spill_with_parent() {
        let body = concat!(
            "- [x] [#b1c4] drop\n",
            "Commands:\n",
            "  cargo test -p agent-doc backlog::\n"
        );
        let (_new_body, removed) = reap_with_items(body).unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].id, "b1c4");
        assert_eq!(
            removed[0].continuation,
            "Commands:\n  cargo test -p agent-doc backlog::\n"
        );
    }

    #[test]
    fn detect_reorder_same_set_different_order() {
        let snap = "- [ ] [#a1b2] one\n- [ ] [#c3d4] two\n";
        let cur = "- [ ] [#c3d4] two\n- [ ] [#a1b2] one\n";
        let result = detect_reorder(snap, cur);
        assert_eq!(result, Some(vec!["c3d4".to_string(), "a1b2".to_string()]));
    }

    #[test]
    fn detect_reorder_none_when_sets_differ() {
        let snap = "- [ ] [#a1b2] one\n";
        let cur = "- [ ] [#a1b2] one\n- [ ] [#c3d4] two\n";
        assert_eq!(detect_reorder(snap, cur), None);
    }

    #[test]
    fn detect_reorder_none_when_same_order() {
        let snap = "- [ ] [#a1b2] one\n- [ ] [#c3d4] two\n";
        assert_eq!(detect_reorder(snap, snap), None);
    }

    #[test]
    fn op_add_inserts_new_item_with_hash() {
        let body = "";
        let (new_body, id) = op_add(body, "first task", DOC_ID, false).unwrap();
        assert!(new_body.contains("- [ ] [#"));
        assert!(new_body.contains("first task"));
        assert!(!id.is_empty());
    }

    #[test]
    fn op_add_prepends_before_existing_items() {
        let body = "- [ ] [#a1b2] existing task\n- [ ] [#c3d4] later task\n";
        let (new_body, _id) = op_add(body, "new first task", DOC_ID, false).unwrap();
        let lines: Vec<&str> = new_body.lines().collect();
        assert!(
            lines[0].contains("new first task"),
            "expected new item first, got: {}",
            new_body
        );
        assert!(
            lines[1].contains("existing task"),
            "expected previous first item second, got: {}",
            new_body
        );
    }

    #[test]
    fn op_add_identical_active_item_is_idempotent() {
        let body = "- [ ] [#a1b2] deploy\n";
        let outcome = op_add_with_outcome(body, "deploy", DOC_ID, false).unwrap();

        assert_eq!(outcome.body, body);
        assert_eq!(outcome.id, "a1b2");
        assert!(!outcome.inserted);
        assert!(outcome.deduped_key.is_none());
    }

    #[test]
    fn op_add_at_after_inserts_immediately_after_anchor() {
        // #ah0s: --pending-add-after lands directly below the anchor.
        let body = "- [ ] [#a1b2] first\n- [ ] [#c3d4] third\n";
        let (new_body, _id) =
            op_add_at(body, "second", DOC_ID, false, AddPosition::After("a1b2")).unwrap();
        let lines: Vec<&str> = new_body.lines().collect();
        assert!(lines[0].contains("first"), "{new_body}");
        assert!(lines[1].contains("second"), "{new_body}");
        assert!(lines[2].contains("third"), "{new_body}");
    }

    #[test]
    fn op_add_at_before_inserts_immediately_before_anchor() {
        let body = "- [ ] [#a1b2] first\n- [ ] [#c3d4] third\n";
        let (new_body, _id) =
            op_add_at(body, "zeroth", DOC_ID, false, AddPosition::Before("a1b2")).unwrap();
        let lines: Vec<&str> = new_body.lines().collect();
        assert!(lines[0].contains("zeroth"), "{new_body}");
        assert!(lines[1].contains("first"), "{new_body}");
    }

    #[test]
    fn op_add_at_last_appends_at_tail() {
        // #ah0s: --pending-add-back lands at the tail without disturbing the head.
        let body = "- [ ] [#a1b2] first\n- [ ] [#c3d4] second\n";
        let (new_body, _id) =
            op_add_at(body, "tail item", DOC_ID, false, AddPosition::Last).unwrap();
        let lines: Vec<&str> = new_body.lines().collect();
        assert!(lines[0].contains("first"), "{new_body}");
        assert!(lines[2].contains("tail item"), "{new_body}");
    }

    #[test]
    fn op_add_at_after_chains_build_deterministic_order() {
        // #ah0s: chaining after A then after B builds A→B→C with no reorder pass.
        let body = "- [ ] [#a1b2] A\n";
        let (b1, _) =
            op_add_at(body, "id=bbbb B", DOC_ID, false, AddPosition::After("a1b2")).unwrap();
        let (b2, _) = op_add_at(&b1, "C", DOC_ID, false, AddPosition::After("bbbb")).unwrap();
        let lines: Vec<&str> = b2.lines().collect();
        assert!(lines[0].contains(" A"), "{b2}");
        assert!(lines[1].contains(" B"), "{b2}");
        assert!(lines[2].contains(" C"), "{b2}");
    }

    #[test]
    fn op_add_at_unknown_anchor_errors() {
        let body = "- [ ] [#a1b2] first\n";
        let err = op_add_at(body, "x", DOC_ID, false, AddPosition::After("nope")).unwrap_err();
        assert!(err.to_string().contains("anchor id not found"), "{err}");
    }

    #[test]
    fn op_add_preserves_section_headers() {
        let body = concat!(
            "### Active\n",
            "- [ ] [#a1b2] existing task\n",
            "### Later\n",
            "- [ ] [#c3d4] later task\n"
        );
        let (new_body, _id) = op_add(body, "new first task", DOC_ID, false).unwrap();
        let lines: Vec<&str> = new_body.lines().collect();
        assert_eq!(lines[0], "### Active");
        assert!(lines[1].contains("new first task"), "got: {}", new_body);
        assert!(lines[2].contains("existing task"), "got: {}", new_body);
        assert_eq!(lines[3], "### Later");
        assert!(lines[4].contains("later task"), "got: {}", new_body);
    }

    #[test]
    fn op_append_items_preserves_order_after_existing_items() {
        let body = concat!(
            "### Active\n",
            "- [ ] [#a1b2] existing task\n",
            "\n",
            "### Notes\n",
            "Keep this note.\n",
        );
        let appended = vec![
            PendingItem {
                marker: PendingListMarker::Bullet,
                id: "b2c3".to_string(),
                state: PendingState::Open,
                gate_type: None,
                in_progress: false,
                text: "first appended task".to_string(),
                continuation: String::new(),
            },
            PendingItem {
                marker: PendingListMarker::Bullet,
                id: "d4e5".to_string(),
                state: PendingState::Gated,
                gate_type: Some("release".to_string()),
                in_progress: false,
                text: "second appended task".to_string(),
                continuation: String::new(),
            },
        ];

        let new_body = op_append_items(body, &appended);

        assert_eq!(
            new_body,
            concat!(
                "### Active\n",
                "- [ ] [#a1b2] existing task\n",
                "- [ ] [#b2c3] first appended task\n",
                "- [/release] [#d4e5] second appended task\n",
                "\n",
                "### Notes\n",
                "Keep this note.\n",
            )
        );
    }

    #[test]
    fn op_add_renumbers_ordered_lists() {
        let body = "1. [ ] [#a1b2] existing task\n2. [ ] [#c3d4] later task\n";
        let (new_body, _id) = op_add(body, "new first task", DOC_ID, false).unwrap();
        let lines: Vec<&str> = new_body.lines().collect();
        assert!(lines[0].starts_with("1. "), "got: {}", new_body);
        assert!(lines[0].contains("new first task"), "got: {}", new_body);
        assert!(lines[1].starts_with("2. "), "got: {}", new_body);
        assert!(lines[1].contains("existing task"), "got: {}", new_body);
        assert!(lines[2].starts_with("3. "), "got: {}", new_body);
        assert!(lines[2].contains("later task"), "got: {}", new_body);
    }

    #[test]
    fn op_add_accepts_custom_id_prefix() {
        let (new_body, id) = op_add("", "id=ship1 release checklist", DOC_ID, false).unwrap();
        assert_eq!(id, "ship1");
        assert!(new_body.contains("- [ ] [#ship1] release checklist"));
    }

    #[test]
    fn op_add_accepts_custom_id_prefix_with_hash_marker() {
        let (new_body, id) = op_add("", "id=#ship1 release checklist", DOC_ID, false).unwrap();
        assert_eq!(id, "ship1");
        assert!(new_body.contains("- [ ] [#ship1] release checklist"));
    }

    #[test]
    fn op_add_accepts_bracketed_custom_id_prefix() {
        let (new_body, id) = op_add("", "[#ship1] release checklist", DOC_ID, false).unwrap();
        assert_eq!(id, "ship1");
        assert!(new_body.contains("- [ ] [#ship1] release checklist"));
    }

    #[test]
    fn op_add_accepts_long_bracketed_custom_id_prefix() {
        let (new_body, id) = op_add("", "[#sdig2matrix] release checklist", DOC_ID, false).unwrap();
        assert_eq!(id, "sdig2matrix");
        assert!(new_body.contains("- [ ] [#sdig2matrix] release checklist"));
    }

    #[test]
    fn op_add_accepts_hyphenated_custom_id_prefix() {
        let (new_body, id) =
            op_add("", "id=tmuxcrash-abcd release checklist", DOC_ID, false).unwrap();
        assert_eq!(id, "tmuxcrash-abcd");
        assert!(new_body.contains("- [ ] [#tmuxcrash-abcd] release checklist"));
    }

    #[test]
    fn op_add_accepts_hyphenated_bracketed_custom_id_prefix() {
        let (new_body, id) =
            op_add("", "[#tmuxcrash-abcd] release checklist", DOC_ID, false).unwrap();
        assert_eq!(id, "tmuxcrash-abcd");
        assert!(new_body.contains("- [ ] [#tmuxcrash-abcd] release checklist"));
    }

    #[test]
    fn op_add_rejects_bare_bracket_placeholder_prefix() {
        let err = op_add("", "[#] release checklist", DOC_ID, false).unwrap_err();
        assert!(format!("{}", err).contains("bare `[#]` placeholder"));
    }

    #[test]
    fn op_add_rejects_empty_explicit_custom_id_prefix() {
        let err = op_add("", "id=  release checklist", DOC_ID, false).unwrap_err();
        assert!(format!("{}", err).contains("empty custom id"));
    }

    #[test]
    fn op_add_rejects_duplicate_custom_id_prefix() {
        let body = "- [ ] [#ship1] existing task\n";
        let err = op_add(body, "id=ship1 new task", DOC_ID, false).unwrap_err();
        assert!(format!("{}", err).contains("custom id already exists"));
    }

    #[test]
    fn op_add_rejects_missing_text_after_custom_id_prefix() {
        let err = op_add("", "id=ship1", DOC_ID, false).unwrap_err();
        assert!(format!("{}", err).contains("custom id prefix must be followed by item text"));
    }

    #[test]
    fn op_add_rejects_missing_text_after_bracketed_custom_id_prefix() {
        let err = op_add("", "[#ship1]", DOC_ID, false).unwrap_err();
        assert!(
            format!("{}", err).contains("bracketed custom id prefix must be followed by item text")
        );
    }

    #[test]
    fn op_add_rejects_stacked_bracketed_custom_id_prefixes() {
        let err = op_add("", "[#ship1] [#ship2] release checklist", DOC_ID, false).unwrap_err();
        assert!(
            format!("{}", err).contains("duplicate leading custom id prefix"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn op_add_rejects_stacked_mixed_custom_id_prefixes() {
        let err = op_add("", "id=ship1 [#ship2] release checklist", DOC_ID, false).unwrap_err();
        assert!(
            format!("{}", err).contains("duplicate leading custom id prefix"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn op_add_rejects_empty() {
        assert!(op_add("", "   ", DOC_ID, false).is_err());
    }

    #[test]
    fn op_add_rejects_state_marker_prefix() {
        for marker in &["[ ] task", "[/] task", "[x] task", "[X] task"] {
            let err = op_add("", marker, DOC_ID, false).unwrap_err();
            let msg = format!("{}", err);
            assert!(
                msg.contains("state marker"),
                "expected state marker error for '{}', got: {}",
                marker,
                msg
            );
        }
    }

    #[test]
    fn op_add_duplicate_text_is_idempotent() {
        let (body, id) = op_add("", "Wire Sift into corky", DOC_ID, false).unwrap();
        let outcome = op_add_with_outcome(&body, "Wire Sift into corky", DOC_ID, false).unwrap();

        assert_eq!(outcome.body, body);
        assert_eq!(outcome.id, id);
        assert!(!outcome.inserted);
        assert!(outcome.deduped_key.is_none());
    }

    #[test]
    fn op_prepend_many_preserves_sequence_order_at_front() {
        let body = "- [ ] [#abcd] existing item\n";
        let outcome = op_prepend_many_with_outcomes(
            body,
            &["first new".to_string(), "second new".to_string()],
            DOC_ID,
            false,
        )
        .unwrap();
        let lines: Vec<&str> = outcome.body.lines().collect();
        assert!(lines[0].contains("first new"), "{}", outcome.body);
        assert!(lines[1].contains("second new"), "{}", outcome.body);
        assert!(lines[2].contains("existing item"), "{}", outcome.body);
        assert_eq!(outcome.outcomes.len(), 2);
        assert!(outcome.outcomes.iter().all(|item| item.inserted));
    }

    #[test]
    fn op_prepend_many_returns_inserted_ids_in_caller_order() {
        let outcome = op_prepend_many_with_outcomes(
            "",
            &[
                "id=first01 first item".to_string(),
                "second auto item".to_string(),
                "[#third03] third item".to_string(),
            ],
            DOC_ID,
            false,
        )
        .unwrap();
        let ids: Vec<&str> = outcome
            .outcomes
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(ids[0], "first01");
        assert!(!ids[1].is_empty());
        assert_eq!(ids[2], "third03");
        let lines: Vec<&str> = outcome.body.lines().collect();
        assert!(
            lines[0].contains("[#first01] first item"),
            "{}",
            outcome.body
        );
        assert!(lines[1].contains("second auto item"), "{}", outcome.body);
        assert!(
            lines[2].contains("[#third03] third item"),
            "{}",
            outcome.body
        );
    }

    #[test]
    fn op_prepend_many_reports_deduped_symptom_outcomes() {
        let key = SymptomDedupeKey::new("stale_queue_pause", "doc-abc", "queue", "sha256:feedface")
            .unwrap();
        let first = format!("stale queue pause {}", key.marker());
        let first_outcome = op_add_with_outcome("", &first, DOC_ID, false).unwrap();
        let repeat = format!("stale queue pause observed again {}", key.marker());
        let outcome = op_prepend_many_with_outcomes(
            &first_outcome.body,
            &["new regular task".to_string(), repeat],
            DOC_ID,
            false,
        )
        .unwrap();
        assert_eq!(outcome.outcomes.len(), 2);
        assert!(outcome.outcomes[0].inserted);
        assert!(!outcome.outcomes[1].inserted);
        assert_eq!(outcome.outcomes[1].id, first_outcome.id);
        assert_eq!(outcome.outcomes[1].deduped_key.as_ref(), Some(&key));
        assert!(
            outcome.body.contains("new regular task"),
            "{}",
            outcome.body
        );
        assert!(
            outcome
                .body
                .contains("  evidence: stale queue pause observed again"),
            "{}",
            outcome.body
        );
    }

    #[test]
    fn op_add_dedupes_symptom_key_by_attaching_evidence() {
        let key = SymptomDedupeKey::new("stale_queue_pause", "doc-abc", "queue", "sha256:feedface")
            .unwrap();
        let first = format!("stale queue pause {}", key.marker());
        let first_outcome = op_add_with_outcome("", &first, DOC_ID, false).unwrap();
        assert!(first_outcome.inserted);

        let repeat = format!("stale queue pause observed again {}", key.marker());
        let repeat_outcome =
            op_add_with_outcome(&first_outcome.body, &repeat, DOC_ID, false).unwrap();

        assert!(!repeat_outcome.inserted);
        assert_eq!(repeat_outcome.id, first_outcome.id);
        assert_eq!(repeat_outcome.deduped_key.as_ref(), Some(&key));
        assert_eq!(repeat_outcome.body.matches("[#").count(), 1);
        assert!(repeat_outcome.body.contains("stale queue pause "));
        assert!(
            repeat_outcome
                .body
                .contains("  evidence: stale queue pause observed again")
        );
    }

    #[test]
    fn parse_bare_hash_placeholder_strips_marker() {
        let item = parse_item_line("- [ ] [#] Wire Sift into corky").unwrap();
        assert_eq!(item.id, "");
        assert_eq!(item.text, "Wire Sift into corky");
    }

    #[test]
    fn backfill_strips_bare_hash_placeholder() {
        let body = "- [ ] [#] task with placeholder\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert!(new_body.contains("[#"), "should have a hash id");
        // The bare [#] should be consumed — only one [# in the output
        let hash_count = new_body.matches("[#").count();
        assert_eq!(hash_count, 1, "expected exactly one [# in: {}", new_body);
        assert!(new_body.contains("task with placeholder"));
        assert!(
            !new_body.contains("[#] task"),
            "bare [#] should not survive in text"
        );
    }

    #[test]
    fn parse_bare_hash_placeholder_no_checkbox() {
        // `- [#] text` — no checkbox, bare placeholder
        let item = parse_item_line("- [#] no checkbox").unwrap();
        assert_eq!(item.id, "");
        assert_eq!(item.state, PendingState::Open);
        assert_eq!(item.text, "no checkbox");
    }

    #[test]
    fn parse_bare_hash_placeholder_gated() {
        // `- [/] [#] gated task` — gated with bare placeholder
        let item = parse_item_line("- [/] [#] gated task").unwrap();
        assert_eq!(item.id, "");
        assert_eq!(item.state, PendingState::Gated);
        assert_eq!(item.text, "gated task");
    }

    #[test]
    fn backfill_strips_multiple_bare_placeholders() {
        // Multiple items each with bare [#] — all should get real IDs
        let body = "- [ ] [#] first task\n- [ ] [#] second task\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items.len(), 2);
        assert!(!items[0].id.is_empty(), "first should have id");
        assert!(!items[1].id.is_empty(), "second should have id");
        assert_ne!(items[0].id, items[1].id, "ids should be unique");
        // No residual [#] in text
        assert!(
            !items[0].text.contains("[#]"),
            "first text has residual [#]: {}",
            items[0].text
        );
        assert!(
            !items[1].text.contains("[#]"),
            "second text has residual [#]: {}",
            items[1].text
        );
    }

    #[test]
    fn backfill_preserves_long_custom_id() {
        let body = "- [ ] [#sdig2matrix] Fixture evidence matrix\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(!changed);
        assert_eq!(new_body, body);
    }

    #[test]
    fn canonicalize_tracked_work_body_backfills_missing_id() {
        let body = "- write focused extraction tests\n";

        let canonical = canonicalize_tracked_work_body(body, DOC_ID);

        assert!(canonical.starts_with("- [ ] [#"), "{canonical}");
        assert!(canonical.contains("write focused extraction tests"));
    }

    #[test]
    fn backfill_idempotent_after_placeholder_strip() {
        // After stripping [#] and assigning ID, second backfill should be a no-op
        let body = "- [ ] [#] task\n";
        let (first_pass, _) = backfill(body, DOC_ID, &ids());
        let (second_pass, changed) = backfill(&first_pass, DOC_ID, &ids());
        assert!(
            !changed,
            "second backfill should be no-op, got: {}",
            second_pass
        );
        assert_eq!(first_pass, second_pass);
    }

    #[test]
    fn op_add_dedup_case_sensitive() {
        // Different casing should NOT be considered duplicate
        let (body, _) = op_add("", "Wire Sift", DOC_ID, false).unwrap();
        let result = op_add(&body, "wire sift", DOC_ID, false);
        assert!(result.is_ok(), "different case should not be duplicate");
    }

    #[test]
    fn op_add_dedup_across_states() {
        // Item exists as gated; adding same text as open is still an active
        // duplicate and should return the existing row idempotently.
        let (body, id) = op_add("", "deploy to prod", DOC_ID, true).unwrap();
        let outcome = op_add_with_outcome(&body, "deploy to prod", DOC_ID, false).unwrap();

        assert_eq!(outcome.body, body);
        assert_eq!(outcome.id, id);
        assert!(!outcome.inserted);
        assert!(outcome.deduped_key.is_none());
    }

    #[test]
    fn op_add_gated_produces_gated_item() {
        let (new_body, id) = op_add("", "gated task", DOC_ID, true).unwrap();
        assert!(new_body.contains("[/]"), "expected [/] in: {}", new_body);
        assert!(new_body.contains(&format!("[#{}]", id)));
        assert!(new_body.contains("gated task"));
    }

    #[test]
    fn op_add_returns_assigned_id() {
        let (body, id1) = op_add("", "task one", DOC_ID, false).unwrap();
        assert!(!id1.is_empty());
        assert!(body.contains(&format!("[#{}]", id1)));
        let (body2, id2) = op_add(&body, "task two", DOC_ID, false).unwrap();
        assert_ne!(id1, id2);
        assert!(body2.contains(&format!("[#{}]", id2)));
    }

    #[test]
    fn op_add_extracts_inline_tag_as_id() {
        let (new_body, id) = op_add(
            "",
            "2026-05-11 [#nopatchbackopencode] OpenCode agent-doc turn completed",
            DOC_ID,
            false,
        )
        .unwrap();
        assert_eq!(id, "nopatchbackopencode");
        assert!(
            new_body.contains(
                "- [ ] [#nopatchbackopencode] 2026-05-11 OpenCode agent-doc turn completed"
            ),
            "got: {}",
            new_body
        );
    }

    #[test]
    fn op_add_inline_tag_strips_from_text() {
        let (new_body, id) = op_add("", "fix [#mybug] the thing", DOC_ID, false).unwrap();
        assert_eq!(id, "mybug");
        assert!(
            new_body.contains("- [ ] [#mybug] fix the thing"),
            "got: {}",
            new_body
        );
    }

    #[test]
    fn op_add_inline_tag_at_end() {
        let (new_body, id) = op_add("", "deploy the thing [#deploy1]", DOC_ID, false).unwrap();
        assert_eq!(id, "deploy1");
        assert!(
            new_body.contains("- [ ] [#deploy1] deploy the thing"),
            "got: {}",
            new_body
        );
    }

    #[test]
    fn op_add_inline_tag_dedup_uses_cleaned_text() {
        let (body, _) = op_add("", "fix [#mybug] the thing", DOC_ID, false).unwrap();
        let outcome = op_add_with_outcome(&body, "fix [#mybug] the thing", DOC_ID, false).unwrap();

        assert_eq!(outcome.body, body);
        assert_eq!(outcome.id, "mybug");
        assert!(!outcome.inserted);
        assert!(outcome.deduped_key.is_none());
    }

    #[test]
    fn op_add_inline_tag_rejects_existing_id() {
        let body = "- [ ] [#mybug] existing task\n";
        let err = op_add(body, "fix [#mybug] new task", DOC_ID, false).unwrap_err();
        assert!(
            format!("{}", err).contains("inline tag id already exists"),
            "expected inline tag id already exists, got: {}",
            err
        );
    }

    #[test]
    fn op_add_inline_tag_ignores_invalid_tag() {
        let (new_body, id) =
            op_add("", "see [#not-a-valid tag] because spaces", DOC_ID, false).unwrap();
        assert_ne!(id, "not-a-valid");
        assert!(
            new_body.contains("see [#not-a-valid tag] because spaces"),
            "got: {}",
            new_body
        );
    }

    #[test]
    fn op_add_leading_prefix_takes_precedence_over_inline() {
        let (new_body, id) =
            op_add("", "[#myid] text with [#other] inline", DOC_ID, false).unwrap();
        assert_eq!(id, "myid");
        assert!(
            new_body.contains("- [ ] [#myid] text with [#other] inline"),
            "got: {}",
            new_body
        );
    }

    #[test]
    fn op_add_promotes_leading_bare_tag_to_id() {
        // Operator-reported gap (#qexplicitid): an agent that adds
        // `#mergestatemachine3 JB+VSCode report…` must get
        // `[#mergestatemachine3] JB+VSCode report…`, not a generated hash with
        // the tag left dangling in the text.
        let (new_body, id) = op_add(
            "",
            "#mergestatemachine3 JB+VSCode report attach/detach through SM",
            DOC_ID,
            false,
        )
        .unwrap();
        assert_eq!(id, "mergestatemachine3");
        assert!(
            new_body
                .contains("- [ ] [#mergestatemachine3] JB+VSCode report attach/detach through SM"),
            "got: {}",
            new_body
        );
    }

    #[test]
    fn op_add_leading_bare_tag_with_trailing_equals_keeps_auto_hash() {
        // A compound topic label like `#lazilyspecpin=Pin …` is NOT an id
        // request — the `=` terminator disqualifies promotion, so the item
        // keeps an auto hash id and the full text is preserved.
        let (new_body, id) =
            op_add("", "#lazilyspecpin=Pin the vocabulary", DOC_ID, false).unwrap();
        assert_ne!(id, "lazilyspecpin");
        assert!(
            new_body.contains("#lazilyspecpin=Pin the vocabulary"),
            "got: {}",
            new_body
        );
    }

    /// A derived id should read like the work it names, not like `#0dsr`.
    #[test]
    fn derive_representative_id_uses_the_items_own_keywords() {
        assert_eq!(
            derive_representative_id(
                "The free text queue items were not immediately replaced by a new backlog item."
            )
            .as_deref(),
            Some("freetextqueue")
        );
        assert_eq!(
            derive_representative_id("We should immediately self-heal, in real time.").as_deref(),
            Some("immediatelyselfheal")
        );
    }

    /// Structural noise must not leak into an id: fenced logs, quoted context,
    /// inline code spans, stopwords, and bare numbers all carry no selectivity.
    #[test]
    fn derive_representative_id_ignores_structural_noise() {
        let text = concat!(
            "Replace the 10 second TTL with a witness.\n",
            "> quoted prior response about caching\n",
            "```\nERROR nonrepresentative log line\n```\n",
        );
        let id = derive_representative_id(text).expect("directive yields an id");
        assert_eq!(id, "replacesecondttl");
        assert!(!id.contains("quoted"), "blockquote context leaked: {id}");
        assert!(!id.contains("error"), "fenced log leaked: {id}");

        let with_code = derive_representative_id("Promote heads using `do [#task-id]` syntax.")
            .expect("directive yields an id");
        assert!(
            !with_code.contains("task"),
            "inline code span leaked: {with_code}"
        );
    }

    /// `#tag` references name OTHER work. Minting one as this item's identity
    /// would invent an id the operator never assigned and collide with the
    /// referenced item — the same invariant `op_add` enforces for bare tags.
    #[test]
    fn derive_representative_id_never_mints_a_referenced_tag_as_the_id() {
        assert_eq!(derive_representative_id("#somereference"), None);
        let id = derive_representative_id("Verify the fix from #somereference regressed nothing.")
            .expect("surrounding prose still yields an id");
        assert!(
            !id.contains("somereference"),
            "referenced tag leaked into the id: {id}"
        );
    }

    /// No distinctive keywords means a surrogate hash is the honest answer.
    #[test]
    fn derive_representative_id_declines_when_text_has_no_keywords() {
        assert_eq!(derive_representative_id(""), None);
        assert_eq!(derive_representative_id("the and but for not"), None);
        assert_eq!(derive_representative_id("agent doc"), None);
    }

    /// Readability must never cost uniqueness.
    #[test]
    fn assign_unique_id_disambiguates_a_taken_representative_id() {
        let mut taken = HashSet::new();
        let text = "Replace the TTL with a liveness witness";
        let first = assign_unique_id(text, DOC_ID, &taken);
        assert_eq!(first, "replacettlliveness");
        taken.insert(first.clone());

        let second = assign_unique_id(text, DOC_ID, &taken);
        assert_ne!(second, first, "a taken id must not be reused");
        assert!(
            second.starts_with("replacettlliveness-"),
            "disambiguation should stay readable: {second}"
        );
    }

    /// Text with no keywords still gets a working surrogate id.
    #[test]
    fn assign_unique_id_falls_back_to_the_surrogate_hash() {
        let taken = HashSet::new();
        let id = assign_unique_id("the and but", DOC_ID, &taken);
        assert_eq!(id, assign_unique_hash("the and but", DOC_ID, &taken));
        assert!(is_valid_pending_id(&id));
    }

    #[test]
    fn op_add_leading_bare_tag_alone_is_reference_not_id() {
        // A bare `#tag` that IS the whole line is a reference, not an id
        // request: it must be left untouched and get an auto hash id.
        let (new_body, id) = op_add("", "#somereference", DOC_ID, false).unwrap();
        assert_ne!(id, "somereference");
        assert!(new_body.contains("#somereference"), "got: {}", new_body);
    }

    #[test]
    fn op_add_leading_bare_tag_rejects_existing_id() {
        let body = "- [ ] [#mergestatemachine3] existing task\n";
        let err = op_add(body, "#mergestatemachine3 new task", DOC_ID, false).unwrap_err();
        assert!(
            format!("{}", err).contains("inline tag id already exists"),
            "expected inline tag id already exists, got: {}",
            err
        );
    }

    #[test]
    fn op_done_marks_checked() {
        let body = "- [ ] [#a1b2] task\n";
        let new_body = op_done(body, "a1b2").unwrap();
        assert!(new_body.contains("[x]"));
    }

    #[test]
    fn op_done_accepts_hash_prefixed_id() {
        let body = "- [ ] [#a1b2] task\n";
        let new_body = op_done(body, "#a1b2").unwrap();
        assert!(new_body.contains("- [x] [#a1b2] task"));
    }

    #[test]
    fn op_done_unknown_id_errors() {
        let body = "- [ ] [#a1b2] task\n";
        assert!(op_done(body, "zzzz").is_err());
    }

    #[test]
    fn op_edit_preserves_hash() {
        let body = "- [ ] [#a1b2] original\n";
        let new_body = op_edit(body, "a1b2", "updated").unwrap();
        assert!(new_body.contains("[#a1b2]"));
        assert!(new_body.contains("updated"));
        assert!(!new_body.contains("original"));
    }

    #[test]
    fn op_edit_many_applies_edits_in_order() {
        let body = "- [ ] [#a1b2] original one\n- [ ] [#c3d4] original two\n";

        let new_body = op_edit_many(
            body,
            &[
                ("a1b2".to_string(), "updated one".to_string()),
                (
                    "c3d4".to_string(),
                    "updated two\n  - retained detail".to_string(),
                ),
            ],
        )
        .unwrap();

        assert!(new_body.contains("- [ ] [#a1b2] updated one"));
        assert!(new_body.contains("- [ ] [#c3d4] updated two\n  - retained detail"));
        assert!(!new_body.contains("original one"));
        assert!(!new_body.contains("original two"));
    }

    #[test]
    fn op_edit_multiline_replaces_existing_continuation() {
        let body = concat!(
            "- [ ] [#tmuxcrash] parent task\n",
            "  - [ ] [#tmuxcrash-old1] stale child\n",
            "  - [ ] [#tmuxcrash-old2] stale child two\n",
            "- [ ] [#keep1] sibling task\n"
        );
        let new_body = op_edit(
            body,
            "tmuxcrash",
            "parent task revised\n  - fresh child\n  - fresh child two",
        )
        .unwrap();
        assert!(new_body.contains("[#tmuxcrash] parent task revised"));
        assert!(new_body.contains("  - fresh child\n"));
        assert!(new_body.contains("  - fresh child two"));
        assert!(!new_body.contains("stale child"));
        assert!(!new_body.contains("stale child two"));
        assert!(new_body.contains("  - fresh child two\n- [ ] [#keep1] sibling task"));
    }

    #[test]
    fn op_edit_rejects_unindented_multiline_follow_up() {
        let body = "- [ ] [#a1b2] original\n";
        let err = op_edit(body, "a1b2", "updated\nsecond parent").unwrap_err();
        assert!(
            format!("{}", err)
                .contains("multiline text may only contain indented continuation lines"),
            "got: {err}"
        );
    }

    #[test]
    fn op_clear_empties_items() {
        let body = "- [ ] [#a1b2] one\n- [ ] [#c3d4] two\n";
        let new_body = op_clear(body).unwrap();
        assert!(!new_body.contains("[#"));
    }

    #[test]
    fn op_clear_preserves_headers_and_spacing() {
        let body = concat!(
            "### Active\n",
            "- [ ] [#a1b2] one\n",
            "\n",
            "### Later\n",
            "- [ ] [#c3d4] two\n"
        );
        let new_body = op_clear(body).unwrap();
        assert_eq!(new_body, "### Active\n\n### Later\n");
    }

    #[test]
    fn op_reorder_reorders_by_id() {
        let body = "- [ ] [#a1b2] first\n- [ ] [#c3d4] second\n- [ ] [#e5f6] third\n";
        let new_body = op_reorder(body, &["e5f6".to_string(), "a1b2".to_string()]).unwrap();
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items[0].id, "e5f6");
        assert_eq!(items[1].id, "a1b2");
        assert_eq!(items[2].id, "c3d4");
    }

    #[test]
    fn op_reorder_keeps_headers_in_place() {
        let body = concat!(
            "### Active\n",
            "- [ ] [#a1b2] first\n",
            "### Later\n",
            "- [ ] [#c3d4] second\n",
            "- [ ] [#e5f6] third\n"
        );
        let new_body = op_reorder(body, &["e5f6".to_string(), "a1b2".to_string()]).unwrap();
        let lines: Vec<&str> = new_body.lines().collect();
        assert_eq!(lines[0], "### Active");
        assert!(lines[1].contains("[#e5f6] third"), "got: {}", new_body);
        assert_eq!(lines[2], "### Later");
        assert!(lines[3].contains("[#a1b2] first"), "got: {}", new_body);
        assert!(lines[4].contains("[#c3d4] second"), "got: {}", new_body);
    }

    #[test]
    fn op_reorder_moves_nested_subtasks_with_parent_item() {
        let body = concat!(
            "- [ ] [#a1b2] first\n",
            "  - child a\n",
            "- [ ] [#c3d4] second\n",
            "  - child b\n"
        );
        let new_body = op_reorder(body, &["c3d4".to_string()]).unwrap();
        assert_eq!(
            new_body,
            concat!(
                "- [ ] [#c3d4] second\n",
                "  - child b\n",
                "- [ ] [#a1b2] first\n",
                "  - child a\n"
            )
        );
    }

    #[test]
    fn op_reorder_renumbers_ordered_lists() {
        let body = "1. [ ] [#a1b2] first\n2. [ ] [#c3d4] second\n3. [ ] [#e5f6] third\n";
        let new_body = op_reorder(body, &["e5f6".to_string(), "a1b2".to_string()]).unwrap();
        let lines: Vec<&str> = new_body.lines().collect();
        assert_eq!(lines[0], "1. [ ] [#e5f6] third");
        assert_eq!(lines[1], "2. [ ] [#a1b2] first");
        assert_eq!(lines[2], "3. [ ] [#c3d4] second");
    }

    #[test]
    fn op_reorder_unknown_id_errors() {
        let body = "- [ ] [#a1b2] one\n";
        assert!(op_reorder(body, &["zzzz".to_string()]).is_err());
    }

    // ---- Phase 2: state matrix + gate/ungate ----

    #[test]
    fn validate_transition_full_matrix() {
        use PendingOp::*;
        use PendingState::*;
        use TransitionResult::*;

        // Open
        assert_eq!(validate_transition(Open, Gate).unwrap(), Transition(Gated));
        assert!(validate_transition(Open, Ungate).is_err());
        assert_eq!(
            validate_transition(Open, MarkDone).unwrap(),
            Transition(Done)
        );

        // Gated
        assert_eq!(validate_transition(Gated, Gate).unwrap(), NoOp);
        assert_eq!(
            validate_transition(Gated, Ungate).unwrap(),
            Transition(Open)
        );
        assert_eq!(
            validate_transition(Gated, MarkDone).unwrap(),
            Transition(Done)
        );

        // Done
        assert!(validate_transition(Done, Gate).is_err());
        assert!(validate_transition(Done, Ungate).is_err());
        assert_eq!(validate_transition(Done, MarkDone).unwrap(), NoOp);
    }

    #[test]
    fn op_gate_open_to_gated() {
        let body = "- [ ] [#a1b2] task\n";
        let new_body = op_gate(body, "a1b2").unwrap();
        assert!(new_body.contains("- [/] [#a1b2]"));
    }

    #[test]
    fn op_gate_gated_is_noop() {
        let body = "- [/] [#a1b2] task\n";
        let new_body = op_gate(body, "a1b2").unwrap();
        // No-op: body unchanged byte-for-byte.
        assert_eq!(new_body, body);
    }

    #[test]
    fn op_gate_done_errors() {
        let body = "- [x] [#a1b2] task\n";
        let err = op_gate(body, "a1b2").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("cannot gate Done item"), "got: {}", msg);
    }

    #[test]
    fn op_gate_unknown_id_errors() {
        let body = "- [ ] [#a1b2] task\n";
        assert!(op_gate(body, "zzzz").is_err());
    }

    #[test]
    fn op_ungate_gated_to_open() {
        let body = "- [/] [#a1b2] task\n";
        let new_body = op_ungate(body, "a1b2").unwrap();
        assert!(new_body.contains("- [ ] [#a1b2]"));
    }

    #[test]
    fn op_ungate_open_errors() {
        let body = "- [ ] [#a1b2] task\n";
        let err = op_ungate(body, "a1b2").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("cannot ungate Open"), "got: {}", msg);
    }

    #[test]
    fn op_ungate_done_errors() {
        let body = "- [x] [#a1b2] task\n";
        let err = op_ungate(body, "a1b2").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("cannot ungate Done"), "got: {}", msg);
    }

    #[test]
    fn op_ungate_unknown_id_errors() {
        let body = "- [/] [#a1b2] task\n";
        assert!(op_ungate(body, "zzzz").is_err());
    }

    #[test]
    fn op_gate_preserves_other_items_and_text() {
        let body = "- [ ] [#a1b2] one\n- [ ] [#c3d4] two — gate: v0.32.6\n- [x] [#e5f6] three\n";
        let new_body = op_gate(body, "c3d4").unwrap();
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items[0].state, PendingState::Open);
        assert_eq!(items[1].state, PendingState::Gated);
        assert_eq!(items[1].text, "two — gate: v0.32.6");
        assert_eq!(items[2].state, PendingState::Done);
    }

    #[test]
    fn generate_hash_n_width4_deterministic_and_short() {
        let h = generate_hash_n("text", "doc", 0, 4);
        assert_eq!(h.len(), 4);
        assert_eq!(h, generate_hash_n("text", "doc", 0, 4));
        assert_ne!(h, generate_hash_n("text", "doc", 1, 4));
    }

    #[test]
    fn generate_hash_n_width4_matches_pre_extension_formula() {
        // Width-4 output must be bit-identical to the pre-#14z4 formula so
        // existing docs don't churn their IDs on re-backfill.
        let cases = [
            ("text", "doc", 0u64, "he5f"),
            ("refactor preflight", "abc123", 7, "wkvb"),
            ("", "", 42, "ywpk"),
            ("long text with spaces", "doc_id_long", 99, "mpy2"),
        ];
        for (t, d, c, expected) in cases {
            assert_eq!(generate_hash_n(t, d, c, 4), expected);
        }
    }

    #[test]
    fn generate_hash_n_widths_have_correct_length() {
        for w in 4..=8 {
            let h = generate_hash_n("text", "doc", 0, w);
            assert_eq!(h.len(), w, "width {} produced len {}", w, h.len());
        }
        // Out-of-range widths clamp to [4, 8].
        assert_eq!(generate_hash_n("x", "y", 0, 1).len(), 4);
        assert_eq!(generate_hash_n("x", "y", 0, 20).len(), 8);
    }

    #[test]
    fn generate_hash_n_wider_extends_shorter() {
        // A wider hash must start with the shorter hash as a prefix so
        // visible widening is explainable to humans.
        let h4 = generate_hash_n("text", "doc", 0, 4);
        let h5 = generate_hash_n("text", "doc", 0, 5);
        let h8 = generate_hash_n("text", "doc", 0, 8);
        assert!(h5.starts_with(&h4), "h5={} h4={}", h5, h4);
        assert!(h8.starts_with(&h4), "h8={} h4={}", h8, h4);
        assert!(h8.starts_with(&h5), "h8={} h5={}", h8, h5);
    }

    #[test]
    fn assign_unique_hash_extends_on_collision() {
        // Pre-populate `taken` with the width-4 hash of "item". The next
        // assignment for the same text must either reuse the width-4 slot
        // with a different counter OR widen. Either way the result must
        // differ from the pre-populated value and be valid.
        let h4 = generate_hash_n("item", "doc", 0, 4);
        let mut taken = HashSet::new();
        taken.insert(h4.clone());
        let id = assign_unique_hash("item", "doc", &taken);
        assert_ne!(id, h4);
        assert!((4..=8).contains(&id.len()));
    }

    #[test]
    fn assign_unique_hash_widens_when_counter_exhausted_at_width4() {
        // Pre-populate `taken` with EVERY width-4 hash the retry loop would
        // try at counters 0..=3. Assignment must widen to 5 chars.
        let mut taken = HashSet::new();
        for c in 0..=3u64 {
            taken.insert(generate_hash_n("x", "d", c, 4));
        }
        let id = assign_unique_hash("x", "d", &taken);
        assert!(!taken.contains(&id));
        // Either width-4 (an untried counter) or width-5+. Accept both —
        // the important invariant is uniqueness, not forced widening.
        assert!((4..=8).contains(&id.len()));
    }

    #[test]
    fn backfill_assigns_collision_free_ids_under_pressure() {
        // Stress test: backfill 50 items. All must get unique 4..=8-char ids.
        let mut body = String::new();
        for i in 0..50 {
            body.push_str(&format!("- item {}\n", i));
        }
        let (out, changed) = backfill(&body, "doc", &HashSet::new());
        assert!(changed);
        let (_, items, _) = parse_items(&out);
        assert_eq!(items.len(), 50);
        let ids: HashSet<String> = items.iter().map(|i| i.id.clone()).collect();
        assert_eq!(ids.len(), 50, "ids must be unique");
        for id in &ids {
            assert!(
                (4..=8).contains(&id.len()),
                "id {} has width {}",
                id,
                id.len()
            );
        }
    }

    // ---- Typed gates ----

    #[test]
    fn parse_typed_gate_release() {
        let body = "- [/release] [#a1b2] Release v0.32.4\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].state, PendingState::Gated);
        assert_eq!(items[0].gate_type, Some("release".to_string()));
        assert_eq!(items[0].text, "Release v0.32.4");
    }

    #[test]
    fn parse_typed_gate_deploy() {
        let body = "- [/deploy] [#c3d4] Push CDN config\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].state, PendingState::Gated);
        assert_eq!(items[0].gate_type, Some("deploy".to_string()));
    }

    #[test]
    fn parse_untyped_gate_has_no_gate_type() {
        let body = "- [/] [#a1b2] waiting\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].state, PendingState::Gated);
        assert_eq!(items[0].gate_type, None);
    }

    #[test]
    fn parse_open_has_no_gate_type() {
        let body = "- [ ] [#a1b2] task\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].gate_type, None);
    }

    #[test]
    fn render_typed_gate() {
        let item = PendingItem {
            marker: PendingListMarker::Bullet,
            id: "a1b2".to_string(),
            state: PendingState::Gated,
            gate_type: Some("release".to_string()),
            in_progress: false,
            text: "Release v0.32.4".to_string(),
            continuation: String::new(),
        };
        assert_eq!(item.render(), "- [/release] [#a1b2] Release v0.32.4");
    }

    #[test]
    fn render_roundtrip_typed_gate() {
        let body = "- [/release] [#a1b2] Release v0.32.4\n- [/deploy] [#c3d4] Push\n- [/] [#e5f6] Generic\n";
        let (p, items, post) = parse_items(body);
        let out = render_items(&p, &items, &post);
        assert_eq!(out, body);
    }

    #[test]
    fn op_resolve_gate_flips_matching() {
        let body = "- [/release] [#a1b2] Release v0.32.4\n- [/deploy] [#c3d4] Deploy\n- [/] [#e5f6] Generic gate\n";
        let (new_body, resolved) = op_resolve_gate(body, "release");
        assert_eq!(resolved, vec!["a1b2"]);
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items[0].state, PendingState::Done); // was [/release]
        assert_eq!(items[0].gate_type, None); // cleared
        assert_eq!(items[1].state, PendingState::Gated); // [/deploy] untouched
        assert_eq!(items[1].gate_type, Some("deploy".to_string()));
        assert_eq!(items[2].state, PendingState::Gated); // [/] untouched
        assert_eq!(items[2].gate_type, None);
    }

    #[test]
    fn op_resolve_gate_no_match() {
        let body = "- [/release] [#a1b2] Release\n- [/] [#c3d4] Generic\n";
        let (new_body, resolved) = op_resolve_gate(body, "deploy");
        assert!(resolved.is_empty());
        assert_eq!(new_body, body);
    }

    #[test]
    fn op_resolve_gate_ignores_untyped() {
        let body = "- [/] [#a1b2] Generic gate\n";
        let (_, resolved) = op_resolve_gate(body, "release");
        assert!(resolved.is_empty());
    }

    #[test]
    fn op_resolve_gate_multiple_same_type() {
        let body = "- [/release] [#a1b2] First\n- [/release] [#c3d4] Second\n";
        let (_, resolved) = op_resolve_gate(body, "release");
        assert_eq!(resolved, vec!["a1b2", "c3d4"]);
    }

    #[test]
    fn op_set_gate_type_on_gated() {
        let body = "- [/] [#a1b2] Release v0.32.4\n";
        let new_body = op_set_gate_type(body, "a1b2", "release").unwrap();
        assert!(new_body.contains("[/release]"));
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items[0].gate_type, Some("release".to_string()));
    }

    #[test]
    fn op_set_gate_type_errors_on_open() {
        let body = "- [ ] [#a1b2] task\n";
        assert!(op_set_gate_type(body, "a1b2", "release").is_err());
    }

    #[test]
    fn op_set_gate_verify_round_trips_predicate() {
        let body = "- [/] [#saev] early receipt live verify\n";
        let new_body = op_set_gate_verify(
            body,
            "saev",
            "verify=ops_log:early_receipt_accepted;disproof=false receipt-timeout",
            1749526200,
        )
        .unwrap();
        let (_, items, _) = parse_items(&new_body);
        // Still gated, untyped checkbox preserved.
        assert_eq!(items[0].state, PendingState::Gated);
        assert_eq!(items[0].gate_type, None);
        let pred = crate::gate_verify::parse_gate_predicate(&items[0].text).unwrap();
        assert_eq!(pred.verify.as_deref(), Some("early_receipt_accepted"));
        assert_eq!(pred.disproof.as_deref(), Some("false receipt-timeout"));
        assert_eq!(pred.set_at, Some(1749526200));
    }

    #[test]
    fn op_set_gate_verify_errors_on_open() {
        let body = "- [ ] [#a1b2] task\n";
        assert!(op_set_gate_verify(body, "a1b2", "verify=ops_log:m", 1).is_err());
    }

    #[test]
    fn op_set_gate_verify_errors_on_empty_spec() {
        let body = "- [/] [#a1b2] task\n";
        assert!(op_set_gate_verify(body, "a1b2", "noop=1", 1).is_err());
    }

    #[test]
    fn op_set_gate_type_errors_on_done() {
        let body = "- [x] [#a1b2] task\n";
        assert!(op_set_gate_type(body, "a1b2", "release").is_err());
    }

    #[test]
    fn op_set_gate_type_replaces_existing() {
        let body = "- [/release] [#a1b2] task\n";
        let new_body = op_set_gate_type(body, "a1b2", "deploy").unwrap();
        assert!(new_body.contains("[/deploy]"));
        assert!(!new_body.contains("[/release]"));
    }

    #[test]
    fn parse_typed_gate_case_insensitive() {
        let body = "- [/Release] [#a1b2] task\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].gate_type, Some("release".to_string()));
    }

    #[test]
    fn parse_typed_gate_with_hyphens_underscores() {
        let body =
            "- [/code-review] [#a1b2] Review PR\n- [/pre_release] [#c3d4] Pre-release check\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].gate_type, Some("code-review".to_string()));
        assert_eq!(items[1].gate_type, Some("pre_release".to_string()));
    }

    #[test]
    fn detect_shadow_open_items_classifies_duplicate_and_shadow_only_ids() {
        let doc = concat!(
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#live1] Keep in backlog\n",
            "- [/] [#gate1] Gated live item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- parked copy\n",
            "- [ ] [#live1] Keep in backlog\n",
            "- [ ] [#lost1] Drifted out of backlog\n",
            "- [x] [#done1] Already done\n",
            "-->\n"
        );

        let report = detect_shadow_open_items(doc).unwrap();
        assert_eq!(
            report
                .duplicated_in_live_backlog
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["live1"]
        );
        assert_eq!(
            report
                .shadow_only
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["lost1"]
        );
    }

    #[test]
    fn detect_shadow_open_items_ignores_agent_queue_id_heads() {
        // #qheadsync orphan-shadow: a reaped id's lingering queue head (`[#id]: note`
        // form) or any #mirrorall-mirrored `do [#id]` head must NOT be classified as
        // a shadow "open backlog item outside the live backlog" — the queue is a
        // legitimate component, not stray prose. A genuine shadow item in a commented
        // section is still caught.
        let doc = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#live1] Keep in backlog\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#live1]\n",
            "- [#reaped1]: 0.2.170 installed\n",
            "- do [#reaped2]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- parked copy\n",
            "- [ ] [#lost1] Drifted out of backlog\n",
            "-->\n",
        );

        let report = detect_shadow_open_items(doc).unwrap();
        // No queue head — reaped or mirrored — is flagged.
        assert!(
            report
                .shadow_only
                .iter()
                .all(|item| item.id != "reaped1" && item.id != "reaped2"),
            "reaped/mirrored queue id heads must not be shadow_only: {:?}",
            report.shadow_only
        );
        assert!(
            report.duplicated_in_live_backlog.is_empty(),
            "queue is excluded from the scan, so the mirrored do [#live1] head is not a duplicate: {:?}",
            report.duplicated_in_live_backlog
        );
        // The genuine commented-out shadow item is still caught.
        assert_eq!(
            report
                .shadow_only
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["lost1"]
        );
    }

    #[test]
    fn detect_shadow_open_items_ignores_indented_nested_ids() {
        let doc = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#live1] Parent task\n",
            "  - [ ] [#nested1] Nested checklist item\n",
            "<!-- /agent:backlog -->\n"
        );

        let report = detect_shadow_open_items(doc).unwrap();
        assert!(report.duplicated_in_live_backlog.is_empty());
        assert!(report.shadow_only.is_empty());
    }

    #[test]
    fn extract_items_by_id_preserves_nested_subtasks() {
        let body = concat!(
            "### Active\n",
            "- [ ] [#move1] Parent task\n",
            "  - child dependency\n",
            "- [ ] [#keep1] Keep task\n"
        );
        let (remaining, moved, matched) =
            extract_items_by_id(body, &["move1".to_string()]).unwrap();
        assert_eq!(matched, vec!["move1".to_string()]);
        assert!(remaining.contains("### Active\n"));
        assert!(!remaining.contains("[#move1]"));
        assert!(remaining.contains("[#keep1] Keep task"));
        assert_eq!(
            moved,
            concat!("- [ ] [#move1] Parent task\n", "  - child dependency\n")
        );
    }

    #[test]
    fn extract_items_by_id_preserves_ordered_list_style() {
        let body = concat!(
            "1. [ ] [#move1] Parent task\n",
            "2. [ ] [#keep1] Keep task\n",
            "3. [ ] [#keep2] Keep task two\n"
        );
        let (remaining, moved, matched) =
            extract_items_by_id(body, &["move1".to_string()]).unwrap();
        assert_eq!(matched, vec!["move1".to_string()]);
        assert_eq!(moved, "1. [ ] [#move1] Parent task\n");
        assert_eq!(
            remaining,
            concat!(
                "1. [ ] [#keep1] Keep task\n",
                "2. [ ] [#keep2] Keep task two\n"
            )
        );
    }

    #[test]
    fn detect_shadow_open_items_ignores_icebox_and_code_blocks() {
        let doc = concat!(
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#live1] Keep in backlog\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#cold1] Intentionally parked\n",
            "<!-- /agent:icebox -->\n\n",
            "```md\n",
            "- [ ] [#code1] Example only\n",
            "```\n"
        );

        let report = detect_shadow_open_items(doc).unwrap();
        assert!(report.duplicated_in_live_backlog.is_empty());
        assert!(report.shadow_only.is_empty());
    }

    #[test]
    fn detect_shadow_open_items_ignores_exchange_transcript_items() {
        let doc = concat!(
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ What are #next-steps to implement the planned ipc features?\n\n",
            "### Re: What are #next-steps to implement the planned ipc features?\n\n",
            "1. [#ipc1] finalize the lazily-serde contract so message shapes are stable.\n\n",
            "### Session Summary\n\n",
            "*Compacted.*\n\n",
            "Icebox:\n",
            "- [ ] [#cold1] Intentionally parked\n",
            "- [ ] [#cold2] Still parked\n\n",
            "❯ #code-review\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#ipc2] Live backlog item\n",
            "- [ ] [#cold1] Intentionally parked\n",
            "- [ ] [#cold2] Still parked\n",
            "<!-- /agent:icebox -->\n"
        );

        let report = detect_shadow_open_items(doc).unwrap();
        assert!(report.duplicated_in_live_backlog.is_empty());
        assert!(report.shadow_only.is_empty());
    }

    #[test]
    fn detect_dropped_from_history_catches_missing_item() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Still here\n",
            "- [ ] [#gone1] Was open in baseline\n",
            "- [ ] [#gone2] Also open in baseline\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Still here\n",
            "<!-- /agent:backlog -->\n"
        );
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        let ids: Vec<&str> = report.dropped.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["gone1", "gone2"]);
    }

    #[test]
    fn detect_dropped_from_history_allows_done_in_live() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] Was open\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "- [x] [#item1] Now done\n",
            "<!-- /agent:backlog -->\n"
        );
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        assert!(report.dropped.is_empty());
    }

    #[test]
    fn detect_dropped_from_history_allows_done_ids() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] Was open\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!("<!-- agent:backlog -->\n", "<!-- /agent:backlog -->\n");
        let mut done = HashSet::new();
        done.insert("item1".to_string());
        let report = detect_dropped_from_history(current, baseline, &done).unwrap();
        assert!(report.dropped.is_empty());
    }

    #[test]
    fn detect_dropped_from_history_allows_completed_archive_id() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] Was open\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n",
            "- 2026-05-10 [#item1] Was open\n",
            "<!-- /agent:done -->\n"
        );
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        assert!(report.dropped.is_empty());
    }

    #[test]
    fn detect_dropped_from_history_rejects_removed_completed_archive_alias() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] Was open\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:pending-done -->\n",
            "- 2026-05-10 [#item1] Was open\n",
            "<!-- /agent:pending-done -->\n"
        );
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        assert_eq!(report.dropped.len(), 1);
        assert_eq!(report.dropped[0].id, "item1");
    }

    #[test]
    fn detect_dropped_from_history_allows_icebox() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] Was open\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#item1] Archived\n",
            "<!-- /agent:icebox -->\n"
        );
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        assert!(report.dropped.is_empty());
    }

    #[test]
    fn detect_dropped_from_history_allows_shadow() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] Was open\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- parked\n",
            "- [ ] [#item1] Drifted to shadow\n",
            "-->\n"
        );
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        assert!(report.dropped.is_empty());
    }

    #[test]
    fn detect_dropped_from_history_ignores_baseline_done_items() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [x] [#done1] Already done in baseline\n",
            "- [/] [#gate1] Gated in baseline\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!("<!-- agent:backlog -->\n", "<!-- /agent:backlog -->\n");
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        let ids: Vec<&str> = report.dropped.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["gate1"]);
    }

    #[test]
    fn detect_dropped_from_history_no_baseline_backlog() {
        let baseline = "# Just a document\nNo backlog here.\n";
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] New item\n",
            "<!-- /agent:backlog -->\n"
        );
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        assert!(report.dropped.is_empty());
    }

    #[test]
    fn detect_dropped_from_history_ignores_code_blocks_in_current() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] Was open\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "```md\n",
            "- [ ] [#item1] In code block only\n",
            "```\n"
        );
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        let ids: Vec<&str> = report.dropped.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["item1"]);
    }

    #[test]
    fn remove_matching_tracked_line_uses_parent_prefix_for_exact_match() {
        let body = "1. [ ] [#one] First\n- [ ] [#two] Second\n  - child detail";

        let (updated, removed) = op_remove_matching_tracked_line(body, "[ ] [#one] First", false);

        assert!(removed);
        assert_eq!(updated, "- [ ] [#two] Second\n  - child detail");
    }

    #[test]
    fn remove_matching_tracked_line_supports_contains_match() {
        let body = "- [ ] [#one] First\n- [ ] [#two] Second";

        let (updated, removed) = op_remove_matching_tracked_line(body, "#two", true);

        assert!(removed);
        assert_eq!(updated, "- [ ] [#one] First");
    }

    #[test]
    fn printable_tracked_work_lines_trims_and_skips_blanks() {
        let body = "\n  - [ ] [#one] First  \n\n  - child detail\n";

        let lines = printable_tracked_work_lines(body);

        assert_eq!(lines, vec!["- [ ] [#one] First", "- child detail"]);
    }

    #[test]
    fn tracked_work_component_lookup_finds_requested_list() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#live1] Live\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#cold1] Parked\n",
            "<!-- /agent:icebox -->\n"
        );

        let component =
            find_tracked_work_component_in_content(content, TrackedWorkList::Icebox).unwrap();

        assert_eq!(component.name, "icebox");
        assert!(component.content(content).contains("[#cold1]"));
    }

    #[test]
    fn open_tracked_work_component_name_ignores_done_items() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [x] [#same1] Done in backlog\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#same1] Open in icebox\n",
            "<!-- /agent:icebox -->\n"
        );

        let component = open_tracked_work_component_name_in_content(content, "same1").unwrap();

        assert_eq!(component.as_deref(), Some("icebox"));
    }

    #[test]
    fn active_identities_include_prompt_presets_and_active_tracked_items() {
        let content = concat!(
            "---\n",
            "prompt_presets:\n",
            "  '#Next-Steps': Any follow-up items?\n",
            "---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] Active backlog item\n",
            "- [x] [#done1] Finished backlog item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#review1] Active review item\n",
            "<!-- /agent:review -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#cold1] Parked item\n",
            "<!-- /agent:icebox -->\n\n",
            "<!-- agent:done -->\n",
            "- 2026-06-30 [#arch1] Archived\n",
            "<!-- /agent:done -->\n"
        );

        let sources = document_active_identities(content);

        assert_eq!(sources.get("next-steps").unwrap(), &vec!["prompt_presets"]);
        assert_eq!(sources.get("alpha").unwrap(), &vec!["agent:backlog"]);
        assert_eq!(sources.get("review1").unwrap(), &vec!["agent:review"]);
        assert_eq!(sources.get("cold1").unwrap(), &vec!["agent:icebox"]);
        assert!(!sources.contains_key("done1"), "{sources:?}");
        assert!(!sources.contains_key("arch1"), "{sources:?}");
    }

    #[test]
    fn detect_identity_collisions_flags_preset_and_tracked_item_ambiguity() {
        let content = concat!(
            "---\n",
            "prompt_presets:\n",
            "  '#next-steps': Any follow-up items?\n",
            "---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#next-steps] Active backlog item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#dup7] Active review item\n",
            "<!-- /agent:review -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#dup7] Parked item\n",
            "<!-- /agent:icebox -->\n"
        );

        let collisions = detect_identity_collisions(content);

        assert_eq!(collisions.len(), 2, "{collisions:?}");
        assert!(
            collisions
                .iter()
                .any(|collision| collision.contains("#dup7")
                    && collision.contains("agent:review + agent:icebox")),
            "{collisions:?}"
        );
        assert!(
            collisions
                .iter()
                .any(|collision| collision.contains("#next-steps")
                    && collision.contains("prompt_presets + agent:backlog")),
            "{collisions:?}"
        );
    }

    #[test]
    fn identity_collision_for_new_id_reports_existing_sources() {
        let content = concat!(
            "---\nprompt_presets:\n  '#next-steps': x\n---\n\n",
            "<!-- agent:backlog -->\n- [ ] [#alpha] active\n<!-- /agent:backlog -->\n"
        );

        assert_eq!(
            identity_collision_for_new_id(content, "next-steps"),
            Some(vec!["prompt_presets".to_string()])
        );
        assert_eq!(
            identity_collision_for_new_id(content, "#ALPHA"),
            Some(vec!["agent:backlog".to_string()])
        );
        assert_eq!(identity_collision_for_new_id(content, "fresh01"), None);
        assert_eq!(identity_collision_for_new_id(content, ""), None);
    }

    #[test]
    fn explicit_new_item_id_collision_reports_existing_active_sources() {
        let content = concat!(
            "---\nprompt_presets:\n  '#next-steps': x\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] Active backlog item\n",
            "<!-- /agent:backlog -->\n"
        );

        assert_eq!(
            explicit_new_item_id_collision(content, "id=NEXT-STEPS add follow-up"),
            Some(ExplicitIdCollision {
                candidate_id: "next-steps".to_string(),
                sources: vec!["prompt_presets".to_string()],
            })
        );
        assert_eq!(
            explicit_new_item_id_collision(content, "[#ALPHA] duplicate"),
            Some(ExplicitIdCollision {
                candidate_id: "alpha".to_string(),
                sources: vec!["agent:backlog".to_string()],
            })
        );
        assert_eq!(
            explicit_new_item_id_collision(content, "mention #alpha without explicit prefix"),
            None
        );
    }

    #[test]
    fn ensure_new_item_explicit_id_available_rejects_ambiguous_insert() {
        let content = concat!(
            "---\nprompt_presets:\n  '#deploy': x\n---\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#deploy] Already active in icebox\n",
            "<!-- /agent:icebox -->\n"
        );

        let err =
            ensure_new_item_explicit_id_available(content, "id=deploy add duplicate").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("#deploy"), "{msg}");
        assert!(msg.contains("prompt_presets + agent:icebox"), "{msg}");
        assert!(msg.contains("#preset-item-id-collision-enforce"), "{msg}");
    }

    #[test]
    fn resolved_tracked_work_id_detects_done_component_and_checked_items() {
        let archive_content = concat!(
            "<!-- agent:done -->\n",
            "- 2026-06-30 [#arch1] Archived\n",
            "<!-- /agent:done -->\n"
        );
        assert!(content_has_resolved_tracked_work_id(archive_content, "ARCH1").unwrap());

        let checked_content = concat!(
            "<!-- agent:backlog -->\n",
            "- [x] [#done1] Done inline\n",
            "<!-- /agent:backlog -->\n"
        );
        assert!(content_has_resolved_tracked_work_id(checked_content, "done1").unwrap());
    }
}
