//! Agent-component overlay on the markdown block tree.
//!
//! Phase 2 of `#md-ast-document-model` (`tasks/agent-doc/plan-md-ast-document-model.md`):
//! layer the `<!-- agent:name -->` … `<!-- /agent:name -->` component model on top
//! of the [`crate`] block tree, exposing each component's list items as **typed
//! nodes with stable ids**. This is the model that lets later phases key
//! consume/dedup/reorder/enqueue on node identity (`[#id]` or content hash) +
//! provenance instead of fragile text-line matching — the root-cause fix for the
//! queue/dedup/live-buffer data-loss family (and directly for
//! `#queue-consume-pushpin-normalization` / `#queue-reconcile-strips-free-text-pins`,
//! since pin/strike state become node attributes, not text prefixes a reconcile
//! can drop).
//!
//! Item parsing is AST-aware only where it matters: fenced-code spans from the
//! block tree are excluded so component markers or `- ` lines *inside* a code
//! block are never mistaken for real items.

use crate::{BlockKind, parse};
use std::hash::{Hash, Hasher};

/// Operator (top-tier) pin spellings, mirrored from the runtime queue model.
const OPERATOR_PIN_MARKERS: [&str; 7] = [
    "**prioritized**",
    "__prioritized__",
    "**pin**",
    "__pin__",
    ":pushpin:",
    ":pin:",
    "📌",
];
/// Agent (middle-tier) pin spellings.
const AGENT_PIN_MARKERS: [&str; 6] = [
    "*prioritized*",
    "_prioritized_",
    "*pin*",
    "_pin_",
    ":round_pushpin:",
    "📍",
];

/// Deterministic, visible separator that introduces the auto-struck explanation
/// note appended *outside* the `~~…~~` strike wrapper (`#qstrikenote`). A struck
/// free-text queue head renders as `~~foo~~ — auto-struck: …`; the separator lets
/// the overlay recognize the line as struck even though it no longer *ends* with
/// `~~`, and lets the consumer detect "already annotated" for idempotency.
pub const STRUCK_ANNOTATION_SEPARATOR: &str = " — auto-struck: ";

/// Split a trimmed item body into `(struck, inner)` where `struck` is true when the
/// body is strikethrough-wrapped. Recognizes both the bare `~~text~~` shape and the
/// annotated `~~text~~ — auto-struck: …` shape (`#qstrikenote`): the trailing note
/// lives outside the wrapper so the line stays readable, but the item is still a
/// struck node whose text is the inner content. Returns `(false, body)` unchanged
/// when the body is not strike-wrapped.
pub(crate) fn split_struck_body(body: &str) -> (bool, &str) {
    let inner_with_tail = match body.strip_prefix("~~") {
        Some(rest) => rest,
        None => return (false, body),
    };
    // Bare wrapper: `~~text~~`.
    if let Some(inner) = inner_with_tail.strip_suffix("~~")
        && !inner.is_empty()
    {
        return (true, inner.trim());
    }
    // Annotated wrapper: `~~text~~ — auto-struck: …`. The closing `~~` is followed
    // by the deterministic annotation separator, so split on the first
    // `~~<separator>` boundary.
    let needle = format!("~~{STRUCK_ANNOTATION_SEPARATOR}");
    if let Some(close) = inner_with_tail.find(&needle) {
        let inner = inner_with_tail[..close].trim();
        if !inner.is_empty() {
            return (true, inner);
        }
    }
    (false, body)
}

/// What a component item *is*, independent of its surface text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemKind {
    /// `do [#id]` / `do #id` — a runnable directive bound to a tracked id.
    Do,
    /// `re [#id]` / `re #id` — a reference to a tracked id, never runnable.
    Reference,
    /// A backlog/review checkbox task: `1. [ ] [#id] …` / `[x]` / `[/]`.
    BacklogTask { checkbox: char },
    /// Free-text prompt (a user question/bug report queued as prose).
    FreeText,
}

/// Surface syntax that owns an item's byte span.
///
/// Queue multiline prompts are logical items even though they are not Markdown
/// list items. Keeping the surface typed lets node mutations render lifecycle
/// changes without guessing from raw strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemSurface {
    ListItem,
    MultilinePrompt,
}

/// A single component item as a typed node: stable id, byte span, surface flags,
/// and kind. `text` is the content with strike/pin markers removed; `raw` keeps
/// the original item content (after the `- ` / `N.` bullet for list items, or
/// the complete fence block for multiline prompts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    /// Whether `id` came from an explicit `[#id]` token in the document.
    ///
    /// Free-text nodes still receive a stable synthetic id for CRDT addressing,
    /// but that identity is not a tracked-work id and must never be presented as
    /// a valid `--done` target.
    pub explicit_id: bool,
    pub text: String,
    pub raw: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub struck: bool,
    pub pinned: bool,
    pub agent_pinned: bool,
    pub kind: ItemKind,
    pub surface: ItemSurface,
}

impl Item {
    /// The row's work text: [`Item::text`] with its leading binary-owned
    /// attempt projection (lifecycle markers, the `[#<owner>#<generation>]`
    /// owner annotation, the `🙋` glyph) removed (`#queuerowworkidentity`).
    ///
    /// `id` is derived from this text, never from the projection, and the
    /// document-cell merge compares it to tell an operator's edit of the work
    /// from a projection-only difference (a stale editor copy carrying an
    /// older or newer marker).
    pub fn work_text(&self) -> &str {
        agent_doc_element_queue::strip_attempt_projection_prefix(&self.text)
    }
}

/// A parsed `<!-- agent:name attrs -->` … `<!-- /agent:name -->` component and
/// its item nodes, in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub name: String,
    pub attrs: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub items: Vec<Item>,
}

/// Byte ranges of fenced/indented code blocks, so markers inside code are ignored.
fn code_ranges(source: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    if let Some(root) = parse(source) {
        collect_code(&root, &mut ranges);
    }
    ranges
}

fn collect_code(block: &crate::Block, out: &mut Vec<(usize, usize)>) {
    if matches!(block.kind, BlockKind::FencedCode | BlockKind::IndentedCode) {
        out.push((block.start_byte, block.end_byte));
    }
    for child in &block.children {
        collect_code(child, out);
    }
}

fn in_code(ranges: &[(usize, usize)], byte: usize) -> bool {
    ranges.iter().any(|&(s, e)| byte >= s && byte < e)
}

/// `#fencedmarkerscan`: the one fence-aware component-marker rule for every
/// line scanner that walks a document's component markers.
///
/// A marker line that starts inside fenced or indented code is content, never
/// an opener or a closer. [`components`] and the document-cell merge's body
/// scan both classify marker lines through this type, so a pasted report whose
/// fence holds a bare `<!-- agent:run ... -->` or `<!-- /agent:run -->` line
/// cannot open a phantom component in one scanner while another keeps a single
/// queue. Offsets are byte offsets into the `source` the scan was built from.
#[derive(Debug, Clone)]
pub struct ComponentMarkerScan {
    code: Vec<(usize, usize)>,
}

impl ComponentMarkerScan {
    pub fn new(source: &str) -> Self {
        Self {
            code: code_ranges(source),
        }
    }

    /// Whether the line starting at `line_start` lies inside code.
    pub fn in_code(&self, line_start: usize) -> bool {
        in_code(&self.code, line_start)
    }

    /// The `(raw_name, attrs)` of a component opener on the line starting at
    /// `line_start` (`line` without its terminator), unless the line is code.
    pub fn open_at(&self, line_start: usize, line: &str) -> Option<(String, String)> {
        if self.in_code(line_start) {
            return None;
        }
        parse_open_marker_line(line.strip_suffix('\r').unwrap_or(line))
    }

    /// The raw name of a component closer (either spelling) on the line
    /// starting at `line_start`, unless the line is code.
    ///
    /// A closer preceded by tolerated stray operator text on the same line
    /// (`t<!-- /agent:run -->`, `#strayclosemarker`) is a closer too; use
    /// [`Self::close_split_at`] to learn where the stray text ends.
    pub fn close_at(&self, line_start: usize, line: &str) -> Option<String> {
        self.close_split_at(line_start, line).map(|(name, _)| name)
    }

    /// The raw name of a component closer on the line starting at
    /// `line_start`, plus the byte length of the stray text before it
    /// (`0` for a standalone closer), unless the line is code.
    ///
    /// `#strayclosemarker`: the element parser and the structural gate read
    /// `t<!-- /agent:run -->` as the content line `t` followed by the closer
    /// (rule owner:
    /// [`agent_doc_element::element::split_tolerated_close_marker_line`]).
    /// A line scanner that missed the welded closer would leave the component
    /// open and swallow the rest of the document into it, so every scanner
    /// that classifies closers goes through here and handles the prefix as
    /// one content line.
    pub fn close_split_at(&self, line_start: usize, line: &str) -> Option<(String, usize)> {
        if self.in_code(line_start) {
            return None;
        }
        if let Some(name) = parse_close_marker(line.trim()) {
            return Some((name, 0));
        }
        let prefix_len = agent_doc_element::element::split_tolerated_close_marker_line(line)?;
        parse_close_marker(line[prefix_len..].trim()).map(|name| (name, prefix_len))
    }
}

/// Parse `<!-- agent:name -->` open markers, returning `(name, attrs)`.
fn parse_open_marker(trimmed: &str) -> Option<(String, String)> {
    let inner = trimmed.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    if inner.contains("-->") {
        // `<!-- agent:x --> trailing -->`: the marker ends at the first `-->`,
        // so the rest is welded text, exactly as the structural gate sees it.
        return None;
    }
    let rest = inner.strip_prefix("agent:")?;
    if rest.starts_with('/') {
        return None; // close marker
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let name = parts.next()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let attrs = parts.next().unwrap_or("").trim().to_string();
    Some((name, attrs))
}

/// Parse a component opener line, returning `(name, attrs)`.
///
/// `line` is one source line without its terminator. The opener either stands
/// alone on the line (surrounding whitespace allowed) or follows an ATX
/// heading with any heading text, e.g.
/// `## Queue: Main <!-- agent:run id="main" subagents -->` (GH #374). The heading rule
/// is owned by [`agent_doc_element::element::component_heading_prefix_matches_tag`],
/// the same predicate the element parser and structural gate use, so the
/// overlay that `respond`/queue mutations resolve components through cannot
/// disagree with `read --component`, `patch`, or `lint` about whether a
/// heading-anchored opener exists. Text after the marker, prose that is not an
/// ATX heading, or a closing tag after a heading is not an opener. The heading
/// text is never part of the component body.
pub fn parse_open_marker_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.starts_with("<!--") {
        return parse_open_marker(trimmed);
    }
    let marker_start = line.find("<!--")?;
    let (name, attrs) = parse_open_marker(line[marker_start..].trim())?;
    let tag = format!("agent:{name}");
    agent_doc_element::element::component_heading_prefix_matches_tag(&line[..marker_start], &tag)
        .then_some((name, attrs))
}

/// Parse a close marker, returning the name. Accepts both spellings:
///   `<!-- /agent:name -->` — the standard slash-then-`agent:name` form
///   `<!-- agent:/name -->` — the legacy `agent:`-then-slash form
fn parse_close_marker(trimmed: &str) -> Option<String> {
    let inner = trimmed.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    let rest = if let Some(after) = inner.strip_prefix('/') {
        // `/agent:name`
        after.strip_prefix("agent:")?
    } else {
        // `agent:/name`
        inner.strip_prefix("agent:")?.strip_prefix('/')?
    };
    let name = rest.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The overlay [`Component::name`] a raw source tag name projects to:
/// the runnable spellings `run` and `loop` resolve to `queue`; every other
/// name is unchanged. Line scanners that match raw open/close tags and then
/// look the component up in [`components`] must map the raw name through this
/// first, or an `agent:run` component is never found.
pub fn projected_component_name(raw_name: &str) -> &str {
    match raw_name {
        "run" | "loop" => "queue",
        other => other,
    }
}

/// Project public runnable-component spellings onto the queue AST model after
/// exact raw open/close matching has succeeded.
fn project_runnable_component(component: &mut Component) {
    if projected_component_name(&component.name) != component.name {
        if component.name == "loop"
            && !component
                .attrs
                .split_whitespace()
                .any(|token| token == "loop" || token.starts_with("loop="))
        {
            component.attrs = if component.attrs.is_empty() {
                "loop".to_string()
            } else {
                format!("{} loop", component.attrs)
            };
        }
        component.name = "queue".to_string();
    }
}

fn is_runnable_component(name: &str) -> bool {
    matches!(name, "queue" | "run" | "loop")
}

/// Strip a leading list bullet (`- `, `* `, `N. `) returning the item content.
pub(crate) fn strip_bullet(line: &str) -> Option<&str> {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
        return Some(rest);
    }
    // ordered list: `12. text`
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() {
        let after = &t[digits.len()..];
        if let Some(rest) = after.strip_prefix(". ") {
            return Some(rest);
        }
    }
    None
}

/// Whether a list item's complete payload is an operator-authored comment.
///
/// Both forms are deliberately line-local. They remain in the source document,
/// but list-backed agent components must not project them as typed work.
pub fn is_ignored_list_comment(item_content: &str) -> bool {
    let trimmed = item_content.trim();

    if trimmed.len() >= 4 && trimmed.starts_with("%%") && trimmed.ends_with("%%") {
        return true;
    }

    let Some(rest) = trimmed.strip_prefix("[//]:") else {
        return false;
    };
    let Some(title) = rest.trim_start().strip_prefix('#') else {
        return false;
    };
    let title = title.trim_start();
    title.starts_with('(') && title.ends_with(')')
}

/// Return the exclusive line index of a balanced indented Obsidian comment
/// whose opening delimiter is the list item at `opener_index`.
pub fn multiline_list_comment_end(
    lines: &[&str],
    opener_index: usize,
    item_indent: usize,
) -> Option<usize> {
    let opener = lines.get(opener_index)?;
    let item_content = strip_bullet(opener)?;
    if item_content.trim() != "%%" {
        return None;
    }

    for (index, line) in lines.iter().enumerate().skip(opener_index + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        if indent <= item_indent {
            return None;
        }
        if trimmed == "%%" {
            return Some(index + 1);
        }
    }
    None
}

fn canonical_response_details_id(item_content: &str) -> Option<&str> {
    let response_id = item_content
        .trim()
        .strip_prefix("<details data-agent-doc-response-id=\"r-")?
        .strip_suffix("\">")?;
    (response_id.len() >= 12
        && response_id
            .chars()
            .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character)))
    .then_some(response_id)
}

fn canonical_bracketed_response_id(value: &str) -> Option<&str> {
    let hash = value.strip_prefix("[#r-")?.strip_suffix(']')?;
    (hash.len() >= 12
        && hash
            .chars()
            .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character)))
    .then_some(value)
}

/// Return the exclusive line index of a reserved GFM response-details child.
///
/// Recognition is deliberately narrow: a canonical generated response id, the
/// fixed summary, and a balanced details element are all required. A completed
/// parent may retain response children from older attempts; every canonical
/// generated response remains inert regardless of which response id the
/// parent's latest backlink names. Ordinary `<details>` blocks remain typed
/// document content.
pub fn response_details_child_end(
    lines: &[&str],
    opener_index: usize,
    item_indent: usize,
) -> Option<usize> {
    let opener = lines.get(opener_index)?;
    canonical_response_details_id(strip_bullet(opener)?)?;

    let summary = lines.get(opener_index + 1)?.trim_end_matches(['\r', '\n']);
    let summary_indent = summary.len() - summary.trim_start_matches([' ', '\t']).len();
    if summary_indent <= item_indent || summary.trim() != "<summary>Response</summary>" {
        return None;
    }

    let mut depth = 1usize;
    for (index, line) in lines.iter().enumerate().skip(opener_index + 2) {
        let line = line.trim_end_matches(['\r', '\n']);
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        if trimmed.starts_with("<details") && trimmed.ends_with('>') {
            depth += 1;
        } else if trimmed == "</details>" {
            depth -= 1;
            if depth == 0 {
                return (indent > item_indent).then_some(index + 1);
            }
        }
    }
    None
}

fn canonical_response_row(item_content: &str) -> Option<(&str, bool)> {
    let body = item_content.trim().strip_prefix("**Response ")?;
    // Parse the summary form first. Its prose may legitimately end in bold
    // text (`... **Operator input needed**`); treating every trailing `**` as
    // the lookup-only form made that generated row executable queue work.
    if let Some((response_id, summary)) = body.split_once(":** ") {
        if summary.trim().is_empty() {
            return None;
        }
        return canonical_bracketed_response_id(response_id).map(|id| (id, true));
    }
    let response_id = body.strip_suffix("**")?;
    canonical_bracketed_response_id(response_id).map(|id| (id, false))
}

fn canonical_response_back_reference(item_content: &str) -> Option<&str> {
    let response_id = item_content.trim().strip_prefix("**Closed by:** ")?;
    canonical_bracketed_response_id(response_id)
}

fn canonical_response_choice(item_content: &str) -> bool {
    let Some(choice) = item_content.trim().strip_prefix("**") else {
        return false;
    };
    let Some((label, effect)) = choice.split_once("**: ") else {
        return false;
    };
    !label.trim().is_empty() && !effect.trim().is_empty()
}

/// Return the exclusive line index of a reserved visible response row.
///
/// The canonical response-id spelling reserves the row as response metadata.
/// A completed parent may retain response children from older attempts; every
/// canonical generated response remains inert regardless of which response id
/// the parent's latest backlink names.
/// The canonical parent row owns only exact generated metadata descendants;
/// unrecognized operator-authored descendants remain ordinary promptable rows.
pub fn response_row_child_end(
    lines: &[&str],
    opener_index: usize,
    _item_indent: usize,
) -> Option<usize> {
    canonical_response_row(strip_bullet(lines.get(opener_index)?)?)?;
    Some(opener_index + 1)
}

fn response_back_reference_child_end(
    lines: &[&str],
    opener_index: usize,
    _item_indent: usize,
) -> Option<usize> {
    canonical_response_back_reference(strip_bullet(lines.get(opener_index)?)?)?;
    Some(opener_index + 1)
}

fn response_metadata_child_end(
    lines: &[&str],
    opener_index: usize,
    item_indent: usize,
    prefix: &str,
) -> Option<usize> {
    let value = strip_bullet(lines.get(opener_index)?)?
        .trim()
        .strip_prefix(prefix)?;
    if value.trim().is_empty() {
        return None;
    }
    let (response_index, response_indent) =
        nearest_prior_line_with_smaller_indent(lines, opener_index, item_indent)?;
    if item_indent != response_indent + 2 {
        return None;
    }
    canonical_response_row(strip_bullet(lines.get(response_index)?)?)?;
    Some(opener_index + 1)
}

fn response_timestamp_child_end(
    lines: &[&str],
    opener_index: usize,
    item_indent: usize,
) -> Option<usize> {
    response_metadata_child_end(lines, opener_index, item_indent, "**Timestamp:** ")
}

fn response_summary_child_end(
    lines: &[&str],
    opener_index: usize,
    item_indent: usize,
) -> Option<usize> {
    response_metadata_child_end(lines, opener_index, item_indent, "**Summary:** ")
}

fn response_state_metadata_child_end(
    lines: &[&str],
    opener_index: usize,
    item_indent: usize,
) -> Option<usize> {
    [
        "**State:** ",
        "**Guidance:** ",
        "**Worktree:** ",
        "**Branch:** ",
        "**Integration:** ",
    ]
    .into_iter()
    .find_map(|prefix| response_metadata_child_end(lines, opener_index, item_indent, prefix))
}

fn nearest_prior_line_with_smaller_indent(
    lines: &[&str],
    opener_index: usize,
    item_indent: usize,
) -> Option<(usize, usize)> {
    lines[..opener_index]
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, line)| {
            let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
            (indent < item_indent).then_some((index, indent))
        })
}

fn response_choices_label_child_end(
    lines: &[&str],
    opener_index: usize,
    item_indent: usize,
) -> Option<usize> {
    if strip_bullet(lines.get(opener_index)?).map(str::trim) != Some("**Choices:**") {
        return None;
    }
    let (response_index, response_indent) =
        nearest_prior_line_with_smaller_indent(lines, opener_index, item_indent)?;
    if item_indent != response_indent + 2 {
        return None;
    }
    // `#waitingpacketowner`: the label is generated metadata anywhere inside a
    // canonical response tree, not only as the response row's direct child.
    // A packet whose Choices drifted one level (beneath its `**Summary:**`)
    // otherwise made every offered option an executable queue leaf, and each
    // mutually exclusive choice was dispatched to its own worker.
    let mut ancestor = Some((response_index, response_indent));
    while let Some((index, indent)) = ancestor {
        let parent = strip_bullet(lines.get(index)?)?;
        if canonical_response_row(parent).is_some()
            || canonical_response_back_reference(parent).is_some()
        {
            return Some(opener_index + 1);
        }
        if indent == 0 {
            break;
        }
        ancestor = nearest_prior_line_with_smaller_indent(lines, index, indent);
    }
    None
}

fn response_next_actions_label_child_end(
    lines: &[&str],
    opener_index: usize,
    item_indent: usize,
) -> Option<usize> {
    if strip_bullet(lines.get(opener_index)?).map(str::trim) != Some("**Next actions:**") {
        return None;
    }
    let (response_index, response_indent) =
        nearest_prior_line_with_smaller_indent(lines, opener_index, item_indent)?;
    if item_indent != response_indent + 2 {
        return None;
    }
    canonical_response_row(strip_bullet(lines.get(response_index)?)?)?;
    Some(opener_index + 1)
}

fn response_decision_metadata_child_end(
    lines: &[&str],
    opener_index: usize,
    item_indent: usize,
) -> Option<usize> {
    let content = strip_bullet(lines.get(opener_index)?)?.trim();
    let value = ["**Context:** ", "**Question:** ", "**Selected:** "]
        .into_iter()
        .find_map(|prefix| content.strip_prefix(prefix))?;
    if value.trim().is_empty() {
        return None;
    }
    let (response_index, response_indent) =
        nearest_prior_line_with_smaller_indent(lines, opener_index, item_indent)?;
    if item_indent != response_indent + 2 {
        return None;
    }
    let (_, has_summary) = canonical_response_row(strip_bullet(lines.get(response_index)?)?)?;
    has_summary.then_some(opener_index + 1)
}

fn response_choice_child_end(
    lines: &[&str],
    opener_index: usize,
    item_indent: usize,
) -> Option<usize> {
    if !canonical_response_choice(strip_bullet(lines.get(opener_index)?)?) {
        return None;
    }
    let (choices_index, choices_indent) =
        nearest_prior_line_with_smaller_indent(lines, opener_index, item_indent)?;
    if item_indent != choices_indent + 2
        || response_choices_label_child_end(lines, choices_index, choices_indent).is_none()
    {
        return None;
    }
    Some(opener_index + 1)
}

fn response_action_child_end(
    lines: &[&str],
    opener_index: usize,
    item_indent: usize,
) -> Option<usize> {
    let action = strip_bullet(lines.get(opener_index)?)?
        .trim()
        .strip_prefix("**Action:** ")?;
    if action.trim().is_empty() {
        return None;
    }
    let (actions_index, actions_indent) =
        nearest_prior_line_with_smaller_indent(lines, opener_index, item_indent)?;
    if item_indent != actions_indent + 2 {
        return None;
    }
    let direct_action =
        response_next_actions_label_child_end(lines, actions_index, actions_indent).is_some();
    let decision_action =
        response_next_action_decision_child_end(lines, actions_index, actions_indent).is_some();
    if !direct_action && !decision_action {
        return None;
    }
    Some(opener_index + 1)
}

fn response_next_action_decision_child_end(
    lines: &[&str],
    opener_index: usize,
    item_indent: usize,
) -> Option<usize> {
    let decision = strip_bullet(lines.get(opener_index)?)?
        .trim()
        .strip_prefix("**Decision:** ")?;
    if decision.trim().is_empty() {
        return None;
    }
    let (actions_index, actions_indent) =
        nearest_prior_line_with_smaller_indent(lines, opener_index, item_indent)?;
    if item_indent != actions_indent + 2
        || response_next_actions_label_child_end(lines, actions_index, actions_indent).is_none()
    {
        return None;
    }
    Some(opener_index + 1)
}

/// Labels the binary renders only as response-structure rows beneath a
/// `**Response [#r-…]**` parent (`#responserowqueuehead`). Labels that take a
/// value end with a space; the container labels stand alone.
const GENERATED_RESPONSE_STRUCTURE_VALUE_LABELS: [&str; 4] = [
    "**Timestamp:** ",
    "**Summary:** ",
    "**State:** ",
    "**Guidance:** ",
];
const GENERATED_RESPONSE_STRUCTURE_CONTAINER_LABELS: [&str; 2] =
    ["**Choices:**", "**Next actions:**"];

/// Whether a list item's content is a generated response-structure row
/// (`**Summary:** …`, `**State:** …`, `**Guidance:** …`, `**Timestamp:** …`,
/// `**Choices:**`, `**Next actions:**`), whatever its parent is.
///
/// `#responserowqueuehead`: on 2026-11-05 a closeout's response tree lost its
/// `**Response [#r-…]**` wrapper on the editor replica, and the orphaned
/// `- **Summary:** …` row was parsed as a free-text prompt, minted a `q-` id,
/// marked 📥, and dispatched as subagent steering. The parent-relative checks
/// above reserve these rows only under a canonical response row, so an orphan
/// fell through to operator work. These labels are the reserved response
/// structure syntax: a row spelled this way is never queue work, with or
/// without its parent. Queue projection (lifecycle markers such as `📥`, a
/// strike wrapper) never makes it work either, so a row already mis-admitted
/// stays inert after the fix. Only the row itself is reserved: an
/// operator-authored child beneath it keeps normal promptability.
pub fn is_generated_response_structure_row(item_content: &str) -> bool {
    let content = item_content.trim();
    let content = agent_doc_element_queue::strip_attempt_projection_prefix(content).trim();
    let (_, inner) = split_struck_body(content);
    let inner = agent_doc_element_queue::strip_attempt_projection_prefix(inner).trim();
    if GENERATED_RESPONSE_STRUCTURE_CONTAINER_LABELS.contains(&inner) {
        return true;
    }
    GENERATED_RESPONSE_STRUCTURE_VALUE_LABELS
        .iter()
        .any(|label| {
            inner
                .strip_prefix(label)
                .is_some_and(|value| !value.trim().is_empty())
        })
}

/// Reserve one orphaned generated response-structure row
/// ([`is_generated_response_structure_row`]) without swallowing any child.
pub fn generated_response_structure_row_end(
    lines: &[&str],
    opener_index: usize,
    _item_indent: usize,
) -> Option<usize> {
    is_generated_response_structure_row(strip_bullet(lines.get(opener_index)?)?)
        .then_some(opener_index + 1)
}

/// Reserved, inert relation metadata for one queue prompt. Relations are
/// navigation edges, never executable queue work and never ordering edges.
/// Only the declaration row is reserved: an operator-authored child beneath it
/// remains an ordinary list row and therefore follows normal promptability.
fn related_row_child_end(
    lines: &[&str],
    opener_index: usize,
    _item_indent: usize,
) -> Option<usize> {
    let body = strip_bullet(lines.get(opener_index)?)?.trim();
    body.strip_prefix("**Related:**")?;
    // The reserved declaration row stays inert even when malformed so a bad
    // relation never turns into executable queue work. Its children are not
    // swallowed: an explicitly authored nested bullet remains promptable.
    Some(opener_index + 1)
}

/// Reserve one agent-doc-generated reply row without swallowing any
/// operator-authored child beneath it. Ownership is decided by the shared
/// [`crate::reply_row::is_agent_doc_reply_row_at`] predicate: a trailing `▫️`
/// in the rendered packet position, or a legacy namespaced comment.
fn agent_doc_reply_child_end(
    lines: &[&str],
    opener_index: usize,
    _item_indent: usize,
) -> Option<usize> {
    crate::reply_row::is_agent_doc_reply_row_at(lines, opener_index).then_some(opener_index + 1)
}

/// Return the end of an inert list child.
///
/// Operator-authored multiline Obsidian comments retain their general list
/// annotation semantics. Generated response projections use only the reserved
/// response-details and visible response-row representations.
pub fn inert_list_child_end(
    lines: &[&str],
    opener_index: usize,
    item_indent: usize,
) -> Option<usize> {
    multiline_list_comment_end(lines, opener_index, item_indent)
        .or_else(|| agent_doc_reply_child_end(lines, opener_index, item_indent))
        .or_else(|| response_details_child_end(lines, opener_index, item_indent))
        .or_else(|| response_row_child_end(lines, opener_index, item_indent))
        .or_else(|| response_back_reference_child_end(lines, opener_index, item_indent))
        .or_else(|| response_timestamp_child_end(lines, opener_index, item_indent))
        .or_else(|| response_summary_child_end(lines, opener_index, item_indent))
        .or_else(|| response_state_metadata_child_end(lines, opener_index, item_indent))
        .or_else(|| response_choices_label_child_end(lines, opener_index, item_indent))
        .or_else(|| response_choice_child_end(lines, opener_index, item_indent))
        .or_else(|| response_next_actions_label_child_end(lines, opener_index, item_indent))
        .or_else(|| response_decision_metadata_child_end(lines, opener_index, item_indent))
        .or_else(|| response_next_action_decision_child_end(lines, opener_index, item_indent))
        .or_else(|| response_action_child_end(lines, opener_index, item_indent))
        .or_else(|| related_row_child_end(lines, opener_index, item_indent))
        // `#treestatusquery`: a `status?` query line and the binary-owned
        // Status reply rows answering it are never queue work.
        .or_else(|| crate::tree_status_row::tree_status_child_end(lines, opener_index))
}

/// Normalize binary-owned reply rows to the bare trailing-glyph form and
/// remove legacy strike wrappers and hidden ownership comments
/// (`#barereplymarker`). Operator-authored descendants and lookalikes remain
/// byte-for-byte unchanged; see [`crate::reply_row::normalize_reply_rows`].
pub fn normalize_agent_doc_reply_rows(source: &str) -> String {
    crate::reply_row::normalize_reply_rows(source)
}

/// Remove queue-attempt lifecycle prefixes accidentally projected onto
/// reserved response-tree rows.
///
/// Response ids and the `data-agent-doc-response-id` attribute reserve these
/// rows as binary-authored metadata. Choices, actions, and relations are
/// recognized only through their response-tree structure. An ordinary child
/// beneath any of those rows is therefore left byte-for-byte promptable.
pub fn normalize_generated_response_lifecycle_markers(source: &str) -> String {
    let lines = source.split_inclusive('\n').collect::<Vec<_>>();
    let normalized = lines
        .iter()
        .map(|line| {
            let Some(body) = strip_bullet(line) else {
                return (*line).to_string();
            };
            let work = agent_doc_element_queue::strip_attempt_projection_prefix(body);
            if work.len() == body.len() {
                return (*line).to_string();
            }
            let body_offset = body.as_ptr() as usize - line.as_ptr() as usize;
            format!("{}{}", &line[..body_offset], work)
        })
        .collect::<Vec<_>>();
    let normalized_lines = normalized.iter().map(String::as_str).collect::<Vec<_>>();

    let mut out = String::with_capacity(source.len());
    for (index, original) in lines.iter().enumerate() {
        let line = normalized_lines[index];
        let item_indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        let generated = response_details_child_end(&normalized_lines, index, item_indent).is_some()
            || agent_doc_reply_child_end(&normalized_lines, index, item_indent).is_some()
            || response_row_child_end(&normalized_lines, index, item_indent).is_some()
            || response_back_reference_child_end(&normalized_lines, index, item_indent).is_some()
            || response_timestamp_child_end(&normalized_lines, index, item_indent).is_some()
            || response_summary_child_end(&normalized_lines, index, item_indent).is_some()
            || response_state_metadata_child_end(&normalized_lines, index, item_indent).is_some()
            || response_choices_label_child_end(&normalized_lines, index, item_indent).is_some()
            || response_choice_child_end(&normalized_lines, index, item_indent).is_some()
            || response_next_actions_label_child_end(&normalized_lines, index, item_indent)
                .is_some()
            || response_decision_metadata_child_end(&normalized_lines, index, item_indent)
                .is_some()
            || response_next_action_decision_child_end(&normalized_lines, index, item_indent)
                .is_some()
            || response_action_child_end(&normalized_lines, index, item_indent).is_some()
            || related_row_child_end(&normalized_lines, index, item_indent).is_some();
        if generated {
            out.push_str(line);
        } else {
            out.push_str(original);
        }
    }
    out
}

/// Byte ranges occupied by balanced indented Obsidian comments represented as
/// list children (`- %%` ... `%%`).
///
/// These ranges are inert queue metadata even when their hidden payload looks
/// like a heading, prompt scaffold, or another list item.
pub fn multiline_list_comment_ranges(source: &str) -> Vec<(usize, usize)> {
    let lines = source.split_inclusive('\n').collect::<Vec<_>>();
    let mut offsets = Vec::with_capacity(lines.len() + 1);
    offsets.push(0);
    for line in &lines {
        offsets.push(offsets.last().copied().unwrap_or_default() + line.len());
    }

    let mut ranges = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let item_indent = lines[index].len() - lines[index].trim_start_matches([' ', '\t']).len();
        if let Some(end) = multiline_list_comment_end(&lines, index, item_indent) {
            ranges.push((offsets[index], offsets[end]));
            index = end;
        } else {
            index += 1;
        }
    }
    ranges
}

/// Byte ranges occupied by inert list children.
pub fn inert_list_child_ranges(source: &str) -> Vec<(usize, usize)> {
    let lines = source.split_inclusive('\n').collect::<Vec<_>>();
    let mut offsets = Vec::with_capacity(lines.len() + 1);
    offsets.push(0);
    for line in &lines {
        offsets.push(offsets.last().copied().unwrap_or_default() + line.len());
    }

    let mut ranges = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let item_indent = lines[index].len() - lines[index].trim_start_matches([' ', '\t']).len();
        if let Some(end) = inert_list_child_end(&lines, index, item_indent) {
            ranges.push((offsets[index], offsets[end]));
            index = end;
        } else {
            index += 1;
        }
    }
    ranges
}

/// Byte ranges occupied specifically by reserved GFM response-details
/// children.
pub fn response_details_child_ranges(source: &str) -> Vec<(usize, usize)> {
    let lines = source.split_inclusive('\n').collect::<Vec<_>>();
    let mut offsets = Vec::with_capacity(lines.len() + 1);
    offsets.push(0);
    for line in &lines {
        offsets.push(offsets.last().copied().unwrap_or_default() + line.len());
    }

    let mut ranges = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let item_indent = lines[index].len() - lines[index].trim_start_matches([' ', '\t']).len();
        if let Some(end) = response_details_child_end(&lines, index, item_indent) {
            ranges.push((offsets[index], offsets[end]));
            index = end;
        } else {
            index += 1;
        }
    }
    ranges
}

/// Byte ranges occupied by reserved visible response rows.
pub fn response_row_child_ranges(source: &str) -> Vec<(usize, usize)> {
    let lines = source.split_inclusive('\n').collect::<Vec<_>>();
    let mut offsets = Vec::with_capacity(lines.len() + 1);
    offsets.push(0);
    for line in &lines {
        offsets.push(offsets.last().copied().unwrap_or_default() + line.len());
    }

    let mut ranges = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let item_indent = lines[index].len() - lines[index].trim_start_matches([' ', '\t']).len();
        if let Some(end) = response_row_child_end(&lines, index, item_indent)
            .or_else(|| response_back_reference_child_end(&lines, index, item_indent))
            .or_else(|| response_timestamp_child_end(&lines, index, item_indent))
            .or_else(|| response_summary_child_end(&lines, index, item_indent))
            .or_else(|| response_state_metadata_child_end(&lines, index, item_indent))
            .or_else(|| response_choices_label_child_end(&lines, index, item_indent))
            .or_else(|| response_choice_child_end(&lines, index, item_indent))
            .or_else(|| response_next_actions_label_child_end(&lines, index, item_indent))
            .or_else(|| response_decision_metadata_child_end(&lines, index, item_indent))
            .or_else(|| response_next_action_decision_child_end(&lines, index, item_indent))
            .or_else(|| response_action_child_end(&lines, index, item_indent))
        {
            ranges.push((offsets[index], offsets[end]));
            index = end;
        } else {
            index += 1;
        }
    }
    ranges
}

/// True when `line` may continue the queue list item directly above it
/// (`#queuecruft`).
///
/// Queue syntax deliberately requires indentation for list-item continuation.
/// CommonMark's unindented "lazy continuation" is ambiguous in an operational
/// queue: operators also use a column-zero paragraph to start the next
/// free-text item. Treating that paragraph as continuation merged two jobs and
/// made the second one impossible to claim (`#queuefreetextsplit`).
///
/// The line-local rule here is the single source of truth shared by this
/// overlay and `document_queue::parse_spans`. A line is NOT a continuation when
/// it is blank, carries its own list marker, is a component/HTML comment marker,
/// opens a fence (```` ``` ````, `~~~`, `---`), is a queue-native directive
/// (`preset`, `dispatch`, a `/command`), is an ATX heading, or is an id-bearing
/// directive (`do [#id]`, `re [#id]`, a bare `[#id]` line) — the last keeps the
/// `#qfoldedhead` re-segmentation of folded `do [#id]` pastes intact.
/// Whether the item above accepts continuation is decided separately by
/// [`queue_item_accepts_continuation`].
pub fn queue_continuation_line(line: &str) -> bool {
    if !line.starts_with([' ', '\t']) {
        return false;
    }
    queue_prompt_content_line(line)
}

/// Stateful boundary policy for continuation text beneath one queue list item.
///
/// Ordinary queue continuation remains indentation-sensitive: column-zero prose
/// starts another free-text head (`#queuefreetextsplit`). Terminal diagnostics,
/// however, commonly start with an indented tree branch and then hard-wrap back
/// to column zero. Once an indented terminal-tree line has been observed, keep
/// subsequent prompt-content lines in the same item until a real queue boundary
/// (blank, list marker, fence, directive, heading, or component marker).
///
/// Keeping this transition state here gives both the typed Markdown overlay and
/// `document_queue::parse_spans` one policy owner (`#queueterminalwrap`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueueContinuationState {
    terminal_tree_evidence: bool,
}

impl QueueContinuationState {
    /// Return whether `line` belongs to the open item and advance the state.
    pub fn accepts_line(&mut self, line: &str) -> bool {
        if queue_continuation_line(line) {
            self.terminal_tree_evidence |= terminal_tree_evidence_line(line);
            return true;
        }
        self.terminal_tree_evidence && queue_bare_prompt_continuation_line(line)
    }
}

fn terminal_tree_evidence_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    ["└", "├", "│", "╰", "╭", "╮", "╯"]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
}

/// True when a column-zero line can start a bare free-text queue paragraph.
/// The paragraph is represented by the same multiline item surface as legacy
/// prompt fences, so node-keyed claim/strike/remove operations have one model.
pub fn queue_bare_prompt_start_line(line: &str) -> bool {
    !line.starts_with([' ', '\t'])
        && !line.trim_start().starts_with('>')
        && queue_prompt_content_line(line)
}

/// True when a line can continue a bare free-text queue paragraph. Unlike a
/// list item, a bare paragraph may own subsequent prose and blockquote lines;
/// blank lines, list markers, thematic breaks, and queue-native syntax stop it.
pub fn queue_bare_prompt_continuation_line(line: &str) -> bool {
    queue_prompt_content_line(line)
}

/// A standalone bare paragraph with an attached blockquote carries enough
/// structure to remain a prompt after the item above it is consumed or reaped.
pub fn queue_bare_prompt_block_has_quote(lines: &[&str], start: usize) -> bool {
    queue_bare_prompt_start_line(lines.get(start).copied().unwrap_or_default())
        && lines
            .iter()
            .skip(start + 1)
            .take_while(|line| queue_bare_prompt_continuation_line(line))
            .any(|line| line.trim_start().starts_with('>'))
}

fn queue_prompt_content_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Own list marker (including an empty `-` / `N.` placeholder and the
    // tolerated stray-backtick `` `- `` mistype).
    if strip_bullet(line).is_some()
        || strip_bullet(trimmed.trim_start_matches('`')).is_some()
        || trimmed.starts_with("+ ")
        || matches!(trimmed, "-" | "*" | "+")
        || trimmed.strip_suffix('.').is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return false;
    }
    if trimmed.starts_with("<!--")
        || trimmed.starts_with("```")
        || trimmed.starts_with("~~~")
        || trimmed.starts_with("---")
        || trimmed == "***"
        || trimmed == "___"
        || trimmed.starts_with("preset ")
        || trimmed.starts_with("dispatch ")
    {
        return false;
    }
    // ATX heading interrupts a paragraph.
    let hashes = trimmed.bytes().take_while(|b| *b == b'#').count();
    if (1..=6).contains(&hashes) && matches!(trimmed.as_bytes().get(hashes), None | Some(b' ')) {
        return false;
    }
    // A `/command` line (but not an absolute path such as `/home/x/y`).
    if let Some(rest) = trimmed.strip_prefix('/') {
        let token = rest.split_whitespace().next().unwrap_or("");
        if !token.is_empty() && !token.contains('/') {
            return false;
        }
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("do [#")
        || lower.starts_with("do #")
        || lower.starts_with("re [#")
        || lower.starts_with("re #")
        || trimmed.starts_with("[#")
    {
        return false;
    }
    true
}

/// True when a queue list item whose content (after the bullet) is
/// `item_content` absorbs following [`queue_continuation_line`]s
/// (`#queuecruft`). Only an unstruck free-text item does: an id-backed
/// `do`/`re` directive never has prose continuation (its indented residue is a
/// folded paste that `#qfoldedhead` re-segments), and a line already struck on
/// its own keeps any bullet-less line below it as separate residue so a
/// completed item can never be resurrected as live text.
pub fn queue_item_accepts_continuation(item_content: &str) -> bool {
    let item = parse_item(item_content, 0, item_content.len(), true);
    !item.struck && queue_item_work_text(&item.text).is_some_and(|work| !work.starts_with("[#"))
}

/// The work text of a parsed queue row for segmentation decisions: the row's
/// text without its leading binary-owned attempt projection (`🚧`, `📥`, `☑️`,
/// `🙋`, `[#subagent:x#1]`, ...), when that work is free text.
///
/// `#nestedsubprompts`: whether a row owns the lines beneath it is a property of the
/// work the operator wrote, never of the projection the binary stamped on it.
/// Deciding on the raw text made `- [#id]` + `  - child` two rows and
/// `- 📥 [#id]` + `  - child` one row, so stamping a marker changed the row
/// count and every marker projection over that queue failed closed with
/// "changed item count" (agent-doc-bugs.md, 2026-10-28).
fn queue_item_work_text(text: &str) -> Option<&str> {
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(text).trim();
    (!work.is_empty() && matches!(classify(work), ItemKind::FreeText)).then_some(work)
}

/// True when `work` opens with a reserved operator-request id
/// (`[#subagent-operator-request-<id>]`).
fn starts_with_operator_request_id(work: &str) -> bool {
    work.strip_prefix("[#")
        .and_then(|rest| rest.split_once(']'))
        .is_some_and(|(id, _)| {
            id.to_ascii_lowercase()
                .starts_with("subagent-operator-request-")
        })
}

/// True when a runnable list row whose content (after its bullet) is
/// Compatibility predicate for callers that still inspect a legacy folded
/// sub-prompt block. Canonical component parsing emits every list row as its
/// own item and does not use this predicate to fold nested rows.
///
/// The decision reads the row's work, not its projection prefix
/// ([`queue_item_work_text`]), so a lifecycle or `🙋` marker never changes
/// which lines a row owns.
///
/// `#queueoperatoranswer`: a row whose work opens with a reserved operator-request id
/// (`- [#subagent-operator-request-x]` with the answer nested beneath it) is
/// the operator's answer to that request, not tracked work. It owns its nested
/// rows exactly as a bare `#subagent-operator-request-x` hashtag parent does,
/// so the answer is one row keyed by the request id and is never offered as a
/// separate free-text head.
pub fn queue_item_accepts_subprompts(item_content: &str) -> bool {
    let item = parse_item(item_content, 0, item_content.len(), true);
    queue_item_work_text(&item.text)
        .is_some_and(|work| !work.starts_with("[#") || starts_with_operator_request_id(work))
}

/// End (exclusive line index) of a legacy foldable sub-prompt block.
///
/// `from` is the first line after the parent row and its `#queuecruft`
/// continuation. The block is the maximal run of lines that are either
///
/// * a list item indented deeper than the parent row whose content is a
///   sub-prompt ([`agent_doc_element_queue::subprompts::subprompt_row_is_eligible`]),
///   or
/// * an indented, non-structural line deeper than the parent (a wrapped
///   child),
///
/// and it ends at a blank line, a row at or above the parent's depth, or any
/// structural line (fence, component marker, heading). `None` when `lines[from]`
/// is not a nested list row, and also when the run reaches a nested row that
/// can NOT be a sub-prompt (a tracked `do [#id]` / bare `[#id]` directive,
/// `preset`, `dispatch`): a subtree with tracked work is a `#f1s3` group, not
/// one prompt, so nothing is folded and the legacy leaf-head shape stands.
///
/// Retained for compatibility helpers and tests; canonical queue enumerators
/// now segment every list row independently.
pub fn queue_subprompt_block_end(lines: &[&str], parent: usize, from: usize) -> Option<usize> {
    use agent_doc_element_queue::subprompts::{
        line_indent_width, list_item_content, subprompt_row_is_eligible,
    };
    let parent_indent = line_indent_width(lines.get(parent)?);
    let first = lines.get(from)?;
    if line_indent_width(first) <= parent_indent || list_item_content(first).is_none() {
        return None;
    }
    let mut end = from;
    while let Some(line) = lines.get(end) {
        if line.trim().is_empty() || line_indent_width(line) <= parent_indent {
            break;
        }
        if let Some(content) = list_item_content(line) {
            if content.trim().is_empty() {
                // An editor placeholder (`  - ` while typing the next child)
                // stays inside the block only when another child follows it;
                // a trailing placeholder is not content (the queue drops it).
                let next_child = lines[end + 1..]
                    .iter()
                    .find(|later| list_item_content(later).is_none_or(|c| !c.trim().is_empty()));
                let continues = next_child.is_some_and(|later| {
                    line_indent_width(later) > parent_indent
                        && list_item_content(later).is_some_and(subprompt_row_is_eligible)
                });
                if !continues {
                    break;
                }
                end += 1;
                continue;
            }
            if !subprompt_row_is_eligible(content) {
                return None;
            }
            end += 1;
            continue;
        }
        if queue_prompt_content_line(line) {
            end += 1;
            continue;
        }
        break;
    }
    (end > from).then_some(end)
}

/// Byte-free shape of one bullet-less multi-line queue head: a prose title run
/// followed by one or more balanced Markdown code fences
/// (`#multilinequeuehead`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueFencedTitleBlock {
    /// Index of the last line that belongs to the head's text (the final fence
    /// closer).
    pub last_text_line: usize,
    /// Index of the last line the head's span owns. Equal to
    /// `last_text_line`, or one trailing `---` separator line after it.
    pub last_line: usize,
}

/// Fence closer for a Markdown code-fence opener line (trimmed): the opener's
/// run of backticks or tildes. The `~~~prompt` / `~~~done` / `~~~start` /
/// `~~~stop` queue surfaces are not code fences.
fn queue_code_fence_run(trimmed: &str) -> Option<(char, usize)> {
    let ch = trimmed.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let run = trimmed.chars().take_while(|c| *c == ch).count();
    if run < 3 {
        return None;
    }
    if ch == '~' && matches!(trimmed, "~~~prompt" | "~~~done" | "~~~start" | "~~~stop") {
        return None;
    }
    // A backtick fence's info string may not contain a backtick.
    if ch == '`' && trimmed[run..].contains('`') {
        return None;
    }
    Some((ch, run))
}

fn queue_code_fence_closes(trimmed: &str, (ch, run): (char, usize)) -> bool {
    trimmed.len() >= run && trimmed.chars().all(|c| c == ch)
}

/// Index of the closer of the balanced code fence opened at `open`, if any.
fn queue_code_fence_close(lines: &[&str], open: usize) -> Option<usize> {
    let fence = queue_code_fence_run(lines.get(open)?.trim())?;
    (open + 1..lines.len()).find(|&j| queue_code_fence_closes(lines[j].trim(), fence))
}

/// Index of the `---` line that closes the bare `---` fence opened at `open`.
///
/// `---` lines inside a balanced code fence (a pasted log) never close it
/// (`#multilinequeuehead`). Shared by `document_queue::parse_spans` and this
/// overlay.
pub fn queue_bare_fence_close(lines: &[&str], open: usize) -> Option<usize> {
    let mut index = open + 1;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if trimmed == "---" {
            return Some(index);
        }
        if let Some(close) = queue_code_fence_close(lines, index) {
            index = close + 1;
            continue;
        }
        index += 1;
    }
    None
}

/// Recognize a bullet-less multi-line queue head starting at `lines[start]`
/// (`#multilinequeuehead`).
///
/// Operators paste diagnostic reports into `agent:queue` as a short title line
/// followed by a fenced log, separated from the previous item by a `---` line
/// or pasted directly beneath the previous multi-line item, without a list
/// bullet or a closing wrapper. Each such block is ONE head:
///
/// - The block must be introduced: the previous non-blank line is a `---`
///   separator, or the caller reports (`after_multiline_item`) that the block
///   directly follows another multi-line item (a `---` / `~~~prompt` /
///   `~~~done` fence or a head of this shape). A lone title plus fence with no
///   such context stays inert pasted evidence unless the legacy
///   `#queue-unwrapped-fenced-task` / `#qfreetext-sep` shapes claim it.
/// - The title run is one or more non-indented prose lines (lines that
///   [`queue_continuation_line`] accepts).
/// - The title run is immediately followed by a balanced code fence (```` ``` ````
///   or `~~~`). Further balanced fences separated only by blank lines belong to
///   the same head.
/// - The head ends at its last fence closer. The next non-blank line must start
///   something else: the end of the body, queue-native syntax (a list item,
///   fence, directive, marker), or another title-plus-fence head. Trailing prose
///   after the fence is not a boundary this rule decides; the caller's legacy
///   shapes (`#queue-unwrapped-fenced-task`, `#qfreetext-sep`) keep owning it.
/// - A single `---` directly after the last fence is the head's trailing
///   separator when what follows it is again a boundary; otherwise it is left
///   in place to open the next `---` fenced prompt.
///
/// This line-level grammar is shared by `document_queue::parse_spans` and this
/// overlay so both enumerators segment the same heads.
pub fn queue_fenced_title_block(
    lines: &[&str],
    start: usize,
    after_multiline_item: bool,
) -> Option<QueueFencedTitleBlock> {
    let is_title_line = |line: &str| {
        queue_bare_prompt_start_line(line) && queue_code_fence_run(line.trim()).is_none()
    };
    if !is_title_line(lines.get(start)?) {
        return None;
    }
    let introduced = after_multiline_item
        || lines[..start]
            .iter()
            .rev()
            .find(|line| !line.trim().is_empty())
            .is_some_and(|line| line.trim() == "---");
    if !introduced {
        return None;
    }
    let mut fence_open = start + 1;
    while fence_open < lines.len() && is_title_line(lines[fence_open]) {
        fence_open += 1;
    }
    let mut last = queue_code_fence_close(lines, fence_open)?;
    loop {
        let mut next = last + 1;
        while next < lines.len() && lines[next].trim().is_empty() {
            next += 1;
        }
        match queue_code_fence_close(lines, next) {
            Some(close) => last = close,
            None => break,
        }
    }
    let next_nonblank = |from: usize| (from..lines.len()).find(|&j| !lines[j].trim().is_empty());
    let is_boundary = |index: Option<usize>| match index {
        None => true,
        Some(j) => {
            !queue_bare_prompt_continuation_line(lines[j])
                || queue_fenced_title_block(lines, j, true).is_some()
        }
    };
    let after = next_nonblank(last + 1);
    if let Some(separator) = after
        && lines[separator].trim() == "---"
    {
        let last_line = if is_boundary(next_nonblank(separator + 1)) {
            separator
        } else {
            last
        };
        return Some(QueueFencedTitleBlock {
            last_text_line: last,
            last_line,
        });
    }
    is_boundary(after).then_some(QueueFencedTitleBlock {
        last_text_line: last,
        last_line: last,
    })
}

fn strip_one_pin(text: &str) -> Option<(&str, bool)> {
    let t = text.trim_start();
    let icons = agent_doc_queue_icons::queue_icons();
    if let Some(rest) = t.strip_prefix(icons.prioritized.as_str()) {
        return Some((rest.trim_start(), true));
    }
    for m in OPERATOR_PIN_MARKERS {
        if let Some(rest) = t.strip_prefix(m) {
            return Some((rest.trim_start(), true));
        }
    }
    if let Some(rest) = t.strip_prefix(icons.agent_prioritized.as_str()) {
        return Some((rest.trim_start(), false));
    }
    for m in AGENT_PIN_MARKERS {
        if let Some(rest) = t.strip_prefix(m) {
            return Some((rest.trim_start(), false));
        }
    }
    None
}

fn extract_bracket_id(text: &str) -> Option<String> {
    let start = text.find("[#")? + 2;
    let end = text[start..].find(']')? + start;
    let id = &text[start..end];
    (!id.is_empty()).then(|| id.to_string())
}

/// A list item's node id and whether it came from an explicit `[#id]`
/// (`#queuerowworkidentity`).
///
/// The id is derived from the row's *work* text, after
/// [`agent_doc_element_queue::strip_attempt_projection_prefix`] removes the
/// binary-owned attempt projection. Keying on the raw first `[#...]` token
/// named a claimed `☑️ [#subagent:x#1] do [#foo]` row `subagent:x#1`, so it
/// and its bare or struck `~~do [#foo]~~` copy were different nodes, and a
/// stale editor copy carrying another marker survived a merge beside (or in
/// place of) the struck row. A row with no projection prefix keys exactly as
/// before: the remainder IS the text.
fn work_item_id(text: &str) -> (String, bool) {
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(text);
    if let Some(id) = agent_doc_element_queue::queue_prompt_stable_id(work)
        && id.starts_with("q-")
    {
        return (id, true);
    }
    if let Some(id) = extract_bracket_id(work) {
        (id, true)
    } else {
        (content_item_id(&classify(work), work), false)
    }
}

fn content_item_id(kind: &ItemKind, text: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{kind:?}").hash(&mut hasher);
    text.trim().hash(&mut hasher);
    format!("ft-{:08x}", hasher.finish() & 0xffff_ffff)
}

fn classify(content: &str) -> ItemKind {
    let t = content.trim_start();
    // Backlog/review checkbox: `[ ] …`, `[x] …`, `[/] …` (bullet already stripped).
    if let Some(rest) = t.strip_prefix('[')
        && rest.len() >= 2
        && rest.as_bytes()[1] == b']'
    {
        return ItemKind::BacklogTask {
            checkbox: rest.as_bytes()[0] as char,
        };
    }
    if t.starts_with("do [#") || t.starts_with("do #") {
        return ItemKind::Do;
    }
    if t.starts_with("re [#") || t.starts_with("re #") {
        return ItemKind::Reference;
    }
    ItemKind::FreeText
}

fn strip_queue_heading(body: &str) -> &str {
    body.trim_start().strip_prefix("### ").unwrap_or(body)
}

fn parse_item(
    raw_line_content: &str,
    start_byte: usize,
    end_byte: usize,
    queue_item: bool,
) -> Item {
    let raw = raw_line_content.trim().to_string();
    // Compatibility for a caller that passes a legacy folded row. Canonical
    // component parsing calls this once per list row, so `subprompt_block` is
    // absent and every child has its own node.
    let (head_raw, subprompt_block) =
        agent_doc_element_queue::subprompts::split_subprompt_block(&raw);
    let mut body = head_raw.trim();

    // Queue navigation headings are presentation state. Canonical rows place
    // `###` before every lifecycle marker (`### ~~…~~`, `### 🚧 …`), while
    // older rows may carry it inside the strike wrapper (`~~### …~~`).
    if queue_item {
        body = strip_queue_heading(body);
    }

    // Strikethrough wrapper. Tolerates an `#qstrikenote` annotation appended
    // outside the `~~…~~` wrapper (`~~text~~ — auto-struck: …`): the line is still
    // a struck node whose text is the inner content.
    let (mut struck, inner) = split_struck_body(body);
    if struck {
        body = inner;
    }
    if queue_item {
        body = strip_queue_heading(body);
    }
    // `#struckparentkids`: a live child written beside struck children under a
    // struck parent reopens the block as one live node (the same rule
    // `document_queue::parse_spans` applies), so the strike/consume path can
    // close it again once the new child is answered.
    if struck
        && subprompt_block
            .is_some_and(agent_doc_element_queue::subprompts::struck_parent_block_reopens)
    {
        struck = false;
    }

    // Pins (may stack, e.g. operator + agent).
    let mut pinned = false;
    let mut agent_pinned = false;
    loop {
        match strip_one_pin(body) {
            Some((rest, true)) => {
                pinned = true;
                body = rest;
            }
            Some((rest, false)) => {
                agent_pinned = true;
                body = rest;
            }
            None => break,
        }
    }

    let kind = classify(body);
    let head_text = body.trim();
    let (id, explicit_id) = work_item_id(head_text);
    let text = match subprompt_block {
        Some(block) => format!("{head_text}\n{}", block.trim_end()),
        None => head_text.to_string(),
    };
    Item {
        id,
        explicit_id,
        text,
        raw,
        start_byte,
        end_byte,
        struck,
        pinned,
        agent_pinned,
        kind,
        surface: ItemSurface::ListItem,
    }
}

fn parse_multiline_item(
    source: &str,
    start_byte: usize,
    raw_end_byte: usize,
    end_byte: usize,
    text: String,
    struck: bool,
) -> Item {
    let raw = source[start_byte..raw_end_byte].to_string();
    let kind = classify(&text);
    // `#multilinequeuehead`: only the head's first line can name a tracked id.
    // A pasted log further down routinely quotes `do [#other]`; reading that as
    // this node's id made a free-text report an explicit-id node keyed after
    // unrelated tracked work.
    //
    // `#queuerowworkidentity`: the id is the work's, not the attempt
    // projection's, so a lifecycle marker or owner annotation leading the
    // title never becomes the node key.
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(&text);
    let first_line = work
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let leading_id = extract_bracket_id(first_line);
    let explicit_id = leading_id.is_some();
    let id = leading_id.unwrap_or_else(|| content_item_id(&classify(work), work));
    Item {
        id,
        explicit_id,
        text,
        raw,
        start_byte,
        end_byte,
        struck,
        pinned: false,
        agent_pinned: false,
        kind,
        surface: ItemSurface::MultilinePrompt,
    }
}

#[derive(Debug, Clone, Copy)]
struct SourceLine<'a> {
    content: &'a str,
    start_byte: usize,
    content_end_byte: usize,
    end_byte: usize,
}

fn source_lines(source: &str, start_byte: usize, end_byte: usize) -> Vec<SourceLine<'_>> {
    let mut lines = Vec::new();
    let mut offset = start_byte;
    for raw in source[start_byte..end_byte].split_inclusive('\n') {
        let without_nl = raw.strip_suffix('\n').unwrap_or(raw);
        let content = without_nl.strip_suffix('\r').unwrap_or(without_nl);
        lines.push(SourceLine {
            content,
            start_byte: offset,
            content_end_byte: offset + content.len(),
            end_byte: offset + raw.len(),
        });
        offset += raw.len();
    }
    lines
}

/// Parse the multiline queue surfaces that `document_queue::parse_spans`
/// recognizes as one logical prompt. This lower AST overlay cannot depend on
/// `agent-doc-queue` (that crate already depends on this one), so the tiny
/// surface grammar lives beside the list-item overlay and is locked to the
/// queue parser by cross-crate integration tests.
fn queue_multiline_items(
    source: &str,
    body_start: usize,
    body_end: usize,
    code_ranges: &[(usize, usize)],
    list_items: &[Item],
) -> Vec<Item> {
    let lines = source_lines(source, body_start, body_end);
    let contents: Vec<&str> = lines.iter().map(|line| line.content).collect();
    let mut items = Vec::new();
    let mut index = 0usize;
    // Bare prose is actionable only at an item boundary. Arbitrary pollution
    // elsewhere in a queue remains inert and recoverable as before.
    let mut bare_prompt_allowed = false;
    // Line index just past the last multi-line item, so a title block pasted
    // directly beneath it (blank lines only between) is introduced by it.
    let mut multiline_end: Option<usize> = None;
    let follows_multiline = |end: Option<usize>, at: usize| {
        end.is_some_and(|end| end <= at && contents[end..at].iter().all(|l| l.trim().is_empty()))
    };
    let title_block_item = |start: usize, from: usize, block: QueueFencedTitleBlock| {
        let text = contents[from..=block.last_text_line].join("\n");
        parse_multiline_item(
            source,
            lines[start].start_byte,
            lines[block.last_line].content_end_byte,
            lines[block.last_line].end_byte,
            text,
            false,
        )
    };
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.content.trim();
        if let Some(item) = list_items
            .iter()
            .find(|item| item.start_byte <= line.start_byte && line.start_byte < item.end_byte)
        {
            if line.end_byte >= item.end_byte {
                bare_prompt_allowed = !item.struck
                    && item.kind == ItemKind::FreeText
                    && !item.text.is_empty()
                    && !item.text.starts_with("[#");
            }
            index += 1;
            continue;
        }
        // `#multilinequeuehead`: a bullet-less title line plus fenced block.
        // Lines a list item already owns (`#queuecruft` continuation) and
        // lines inside code are never a new head.
        if !in_code(code_ranges, line.start_byte)
            && !list_items
                .iter()
                .any(|item| item.start_byte <= line.start_byte && line.start_byte < item.end_byte)
            && let Some(block) =
                queue_fenced_title_block(&contents, index, follows_multiline(multiline_end, index))
        {
            items.push(title_block_item(index, index, block));
            index = block.last_line + 1;
            multiline_end = Some(index);
            bare_prompt_allowed = true;
            continue;
        }
        let fence = match trimmed {
            // A bare thematic-break fence is queue syntax only outside a
            // Markdown code block.
            "---" if !in_code(code_ranges, line.start_byte) => Some(("---", false)),
            // tree-sitter classifies these blocks as fenced code, but in an
            // agent:queue component they are the canonical prompt lifecycle
            // surfaces.
            "~~~prompt" => Some(("~~~", false)),
            "~~~done" => Some(("~~~", true)),
            _ => None,
        };
        let Some((closer, struck)) = fence else {
            // `#queuefreetextsplit`: column-zero prose is a distinct queue
            // head, not CommonMark lazy continuation of the list item above.
            // Keep consecutive prose and an immediately following blockquote
            // together as one paragraph-shaped head.
            if (bare_prompt_allowed || queue_bare_prompt_block_has_quote(&contents, index))
                && !in_code(code_ranges, line.start_byte)
                && queue_bare_prompt_start_line(line.content)
            {
                let mut end = index + 1;
                while end < lines.len()
                    && !in_code(code_ranges, lines[end].start_byte)
                    && queue_bare_prompt_continuation_line(lines[end].content)
                {
                    end += 1;
                }
                let text = contents[index..end].join("\n");
                items.push(parse_multiline_item(
                    source,
                    line.start_byte,
                    lines[end - 1].content_end_byte,
                    lines[end - 1].end_byte,
                    text,
                    false,
                ));
                index = end;
                multiline_end = Some(end);
                bare_prompt_allowed = false;
                continue;
            }
            if trimmed.is_empty() {
                index += 1;
                continue;
            }
            if matches!(trimmed, "***" | "___") {
                index += 1;
                continue;
            }
            bare_prompt_allowed = false;
            index += 1;
            continue;
        };
        let close_index = if closer == "---" {
            queue_bare_fence_close(&contents, index)
        } else {
            (index + 1..lines.len()).find(|candidate| lines[*candidate].content.trim() == closer)
        };
        let Some(close_index) = close_index else {
            // `#multilinequeuehead`: an unclosed `---` directly above a
            // title-plus-fence head is that head's leading separator.
            if closer == "---"
                && let Some(block) = queue_fenced_title_block(&contents, index + 1, true)
            {
                items.push(title_block_item(index, index + 1, block));
                index = block.last_line + 1;
                multiline_end = Some(index);
                bare_prompt_allowed = true;
                continue;
            }
            // Match document_queue's fail-closed treatment of unclosed fences:
            // the opener is inert and later list items remain addressable.
            index += 1;
            continue;
        };
        let text = lines[index + 1..close_index]
            .iter()
            .map(|line| line.content)
            .collect::<Vec<_>>()
            .join("\n");
        if !text.trim().is_empty() {
            items.push(parse_multiline_item(
                source,
                line.start_byte,
                lines[close_index].content_end_byte,
                lines[close_index].end_byte,
                text,
                struck,
            ));
            multiline_end = Some(close_index + 1);
            bare_prompt_allowed = true;
        }
        index = close_index + 1;
    }
    items
}

fn include_queue_multiline_items(
    source: &str,
    component: &mut Component,
    body_end: usize,
    code_ranges: &[(usize, usize)],
) {
    if component.name != "queue" {
        return;
    }
    let body_start = source[component.start_byte..]
        .find('\n')
        .map(|relative| component.start_byte + relative + 1)
        .unwrap_or(body_end)
        .min(body_end);
    let multiline =
        queue_multiline_items(source, body_start, body_end, code_ranges, &component.items);
    if multiline.is_empty() {
        return;
    }
    component.items.retain(|item| {
        !multiline
            .iter()
            .any(|block| item.start_byte < block.end_byte && block.start_byte < item.end_byte)
    });
    component.items.extend(multiline);
    component.items.sort_by_key(|item| item.start_byte);
}

/// Parse all agent-components in `source` into typed nodes, in document order.
/// Markers and list lines inside fenced/indented code are ignored. Queue
/// multiline prompt fences are retained as one typed item spanning the block.
pub fn components(source: &str) -> Vec<Component> {
    let markers = ComponentMarkerScan::new(source);
    let ranges = markers.code.clone();
    let mut out: Vec<Component> = Vec::new();
    let mut open: Option<Component> = None;

    // Walk an indexed line table so continuation and marker ranges retain
    // exact source offsets.
    // `#strayclosemarker`: a closer with tolerated stray text before it
    // (`t<!-- /agent:run -->`) is scanned as two lines, the stray text and then
    // the closer, which is how the element parser places the bytes (the text
    // is the component's last content). The text keeps its exact source
    // offsets; only the line table is split.
    //
    // `#straytextqueueitem`: that stray line is lint-only content. It is never
    // a queue item and never a continuation of the item above it, so a
    // dispatcher addressing items through this overlay cannot select it.
    let mut raw_lines: Vec<&str> = Vec::new();
    let mut stray_close_text_lines: Vec<usize> = Vec::new();
    {
        let mut line_start = 0usize;
        for line in source.split_inclusive('\n') {
            let content = line.trim_end_matches('\n');
            match markers.close_split_at(line_start, content) {
                Some((_, prefix_len)) if prefix_len > 0 => {
                    stray_close_text_lines.push(raw_lines.len());
                    raw_lines.push(&line[..prefix_len]);
                    raw_lines.push(&line[prefix_len..]);
                }
                _ => raw_lines.push(line),
            }
            line_start += line.len();
        }
    }
    let content_lines: Vec<&str> = raw_lines
        .iter()
        .map(|line| {
            let content = line.trim_end_matches('\n');
            content.strip_suffix('\r').unwrap_or(content)
        })
        .collect();
    let mut line_offsets = Vec::with_capacity(raw_lines.len());
    let mut running = 0usize;
    for line in &raw_lines {
        line_offsets.push(running);
        running += line.len();
    }

    let mut offset;
    // `#queuecruft`: byte offset where the open queue list item's content
    // starts, while that item still accepts continuation lines.
    let mut continuable_item: Option<(usize, QueueContinuationState)> = None;
    let mut line_index = 0usize;
    while line_index < raw_lines.len() {
        let index = line_index;
        line_index += 1;
        let line = raw_lines[index];
        let line_start = line_offsets[index];
        offset = line_start + line.len();
        if markers.in_code(line_start) {
            continuable_item = None;
            continue;
        }
        let content = content_lines[index];
        let trimmed = content.trim();

        if stray_close_text_lines.binary_search(&index).is_ok() {
            continuable_item = None;
            continue;
        }

        if let Some((raw_start, state)) = continuable_item.as_mut() {
            if state.accepts_line(content)
                && let Some(comp) = open.as_mut()
                && let Some(last) = comp.items.last_mut()
            {
                let joined = &source[*raw_start..line_start + content.len()];
                *last = parse_item(
                    joined,
                    last.start_byte,
                    offset,
                    is_runnable_component(&comp.name),
                );
                continue;
            }
            continuable_item = None;
        }

        if let Some(name) = markers.close_at(line_start, trimmed) {
            // `#straytextqueueitem`: multiline/bare-prompt recovery scans the
            // body up to, but never into, stray text welded to this closer.
            let body_end = match index.checked_sub(1) {
                Some(previous) if stray_close_text_lines.binary_search(&previous).is_ok() => {
                    line_offsets[previous]
                }
                _ => line_start,
            };
            if let Some(mut comp) = open.take() {
                if comp.name == name {
                    comp.end_byte = offset;
                    project_runnable_component(&mut comp);
                    include_queue_multiline_items(source, &mut comp, body_end, &ranges);
                    out.push(comp);
                    continue;
                }
                // `#componentownclose`: a close that names a different
                // component never ends this one, and this one never extends
                // past it either. Finish it dangling (its `end_byte` stays at
                // the opener line) so rows after a sibling's close are never
                // absorbed into it as items.
                project_runnable_component(&mut comp);
                include_queue_multiline_items(source, &mut comp, body_end, &ranges);
                out.push(comp);
            }
            continue;
        }
        if let Some((name, attrs)) = markers.open_at(line_start, content) {
            // A new open marker closes any dangling component implicitly.
            if let Some(mut comp) = open.take() {
                project_runnable_component(&mut comp);
                include_queue_multiline_items(source, &mut comp, line_start, &ranges);
                out.push(comp);
            }
            open = Some(Component {
                name,
                attrs,
                start_byte: line_start,
                end_byte: offset,
                items: Vec::new(),
            });
            continue;
        }
        if let Some(comp) = open.as_mut()
            && let Some(item_content) = strip_bullet(content)
        {
            let item_indent = content.len() - content.trim_start_matches([' ', '\t']).len();
            if let Some(end_index) = inert_list_child_end(&content_lines, index, item_indent) {
                continuable_item = None;
                line_index = end_index;
                continue;
            }
            // `#responserowqueuehead`: a generated response-structure row is
            // never runnable queue work, even orphaned from its response row.
            if is_runnable_component(&comp.name)
                && let Some(end_index) =
                    generated_response_structure_row_end(&content_lines, index, item_indent)
            {
                continuable_item = None;
                line_index = end_index;
                continue;
            }
            if is_ignored_list_comment(item_content) {
                continue;
            }
            comp.items.push(parse_item(
                item_content,
                line_start,
                offset,
                is_runnable_component(&comp.name),
            ));
            // `item_content` is a suffix slice of `content`, which starts at
            // `line_start`.
            let raw_start = line_start + (content.len() - item_content.len());
            if is_runnable_component(&comp.name) && queue_item_accepts_continuation(item_content) {
                continuable_item = Some((raw_start, QueueContinuationState::default()));
            }
        }
    }
    if let Some(mut comp) = open.take() {
        project_runnable_component(&mut comp);
        include_queue_multiline_items(source, &mut comp, source.len(), &ranges);
        out.push(comp);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "\
<!-- agent:queue priority go -->
- :pushpin: do [#alpha]
- ~~:pushpin: do [#beta]~~
- :pushpin: Free-text bug report about pins
- re [#gamma]
<!-- /agent:queue -->

<!-- agent:backlog priority queue -->
1. [ ] [#alpha] first task
2. [x] [#beta] done task
<!-- /agent:backlog -->
";

    /// `#strayclosemarker`: `t<!-- /agent:run -->` closes its run exactly as
    /// the element parser reads it. The stray text is the run's last content,
    /// never a reason to leave the run open until the next opener.
    #[test]
    fn stray_text_before_a_closer_still_closes_the_component() {
        let source = concat!(
            "## fpe-prs <!-- agent:run id=\"fpe-prs\" subagents go -->\n",
            "- do [#a] task\n",
            "t<!-- /agent:run -->\n",
            "\n",
            "## main <!-- agent:run id=\"main\" subagents go -->\n",
            "- ship it\n",
            "<!-- /agent:run -->\n",
        );
        let element = agent_doc_element::element::parse(source).expect("element parser accepts");
        let parsed = components(source);
        assert_eq!(parsed.len(), 2);
        let first_close_line_end =
            source.find("t<!-- /agent:run -->\n").unwrap() + "t<!-- /agent:run -->\n".len();
        assert_eq!(parsed[0].end_byte, first_close_line_end);
        assert_eq!(element[0].close_end, first_close_line_end);
        assert_eq!(parsed[0].items[0].text, "do [#a] task");
        assert!(
            parsed[0]
                .items
                .iter()
                .all(|item| item.end_byte <= element[0].close_start),
            "no item reaches into the closer: {:?}",
            parsed[0].items
        );
        assert_eq!(
            parsed[1]
                .items
                .iter()
                .map(|item| item.text.as_str())
                .collect::<Vec<_>>(),
            vec!["ship it"]
        );

        let scan = ComponentMarkerScan::new(source);
        let line_start = source.find("t<!-- /agent:run -->").unwrap();
        assert_eq!(
            scan.close_split_at(line_start, "t<!-- /agent:run -->"),
            Some(("run".to_string(), 1))
        );
        assert_eq!(
            scan.close_at(line_start, "t<!-- /agent:run -->").as_deref(),
            Some("run")
        );
        assert_eq!(
            scan.close_at(line_start, "## Done <!-- /agent:run -->"),
            None
        );
    }

    /// `#straytextqueueitem`: stray text welded to a closer is lint-only
    /// content, so the item overlay (which `queue consume` and node-key
    /// addressing use) never exposes it as an item or folds it into the item
    /// above. The same text on its own line is an item.
    #[test]
    fn stray_text_before_a_closer_is_never_an_item() {
        for name in ["run", "queue", "loop"] {
            for stray in ["t", "- t", "  t", "- fix the build"] {
                let welded = format!(
                    "<!-- agent:{name} go -->\n- do [#a] task\n- investigate the route test\n{stray}<!-- /agent:{name} -->\n"
                );
                let items: Vec<String> = components(&welded)[0]
                    .items
                    .iter()
                    .map(|item| item.text.clone())
                    .collect();
                assert_eq!(
                    items,
                    vec!["do [#a] task", "investigate the route test"],
                    "{name} {stray:?}"
                );
            }
            let own_line =
                format!("<!-- agent:{name} go -->\n- do [#a] task\n- t\n<!-- /agent:{name} -->\n");
            let items: Vec<String> = components(&own_line)[0]
                .items
                .iter()
                .map(|item| item.text.clone())
                .collect();
            assert_eq!(items, vec!["do [#a] task", "t"], "{name}");
        }
    }

    #[test]
    fn list_comment_items_are_preserved_but_not_typed() {
        for name in ["run", "loop", "queue", "backlog", "review", "icebox"] {
            let source = format!(
                "<!-- agent:{name} -->\n- parent\n  - [//]: # (markdown note)\n  - %%obsidian note%%\n  - child with %%inline note%%\n  - [//]: # (note) plus work\n<!-- /agent:{name} -->\n"
            );
            let parsed = components(&source);
            assert_eq!(parsed.len(), 1, "component {name}");
            let texts: Vec<&str> = parsed[0]
                .items
                .iter()
                .map(|item| item.text.as_str())
                .collect();
            assert_eq!(
                texts,
                vec![
                    "parent",
                    "child with %%inline note%%",
                    "[//]: # (note) plus work"
                ],
                "component {name}"
            );
            assert!(source.contains("- [//]: # (markdown note)"));
            assert!(source.contains("- %%obsidian note%%"));
        }
    }

    #[test]
    fn multiline_list_comment_body_is_preserved_but_not_typed() {
        let source = concat!(
            "<!-- agent:queue -->\n",
            "- parent\n",
            "  - %%\n",
            "    ### Re: parent\n",
            "\n",
            "    - response bullet, not queue work\n",
            "    %%\n",
            "- next task\n",
            "<!-- /agent:queue -->\n",
        );

        let parsed = components(source);
        let texts = parsed[0]
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(texts, vec!["parent", "next task"]);
        let ranges = multiline_list_comment_ranges(source);
        assert_eq!(ranges.len(), 1);
        assert_eq!(
            &source[ranges[0].0..ranges[0].1],
            concat!(
                "  - %%\n",
                "    ### Re: parent\n",
                "\n",
                "    - response bullet, not queue work\n",
                "    %%\n",
            )
        );
    }

    #[test]
    fn reserved_response_details_child_is_preserved_but_not_typed() {
        let source = concat!(
            "<!-- agent:queue -->\n",
            "- ~~do [#answered]~~ — auto-struck: response: [#r-0123456789ab]\n",
            "  - <details data-agent-doc-response-id=\"r-0123456789ab\">\n",
            "    <summary>Response</summary>\n",
            "    \n",
            "    > **Queue prompt:** copied evidence\n",
            "    ### Re: copied heading\n",
            "    - [ ] copied task-like row\n",
            "    <details>\n",
            "    <summary>Nested</summary>\n",
            "    nested body\n",
            "    </details>\n",
            "    </details>\n",
            "- next task\n",
            "<!-- /agent:queue -->\n",
        );

        let parsed = components(source);
        let texts = parsed[0]
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(texts, vec!["do [#answered]", "next task"]);
        let ranges = response_details_child_ranges(source);
        assert_eq!(ranges.len(), 1);
        assert!(source[ranges[0].0..ranges[0].1].contains("copied task-like row"));
    }

    #[test]
    fn reserved_response_row_and_choices_are_inert_but_operator_reply_is_typed() {
        let source = concat!(
            "<!-- agent:queue -->\n",
            "- ~~do [#answered]~~ — auto-struck: response: [#r-0123456789ab]\n",
            "  - **Response [#r-0123456789ab]:** Deployment is blocked. Which target?\n",
            "    - **Choices:**\n",
            "      - **staging**: Deploy to staging.\n",
            "        - Use staging and run the smoke test.\n",
            "      - **production**: Deploy to production.\n",
            "- next task\n",
            "<!-- /agent:queue -->\n",
        );

        let parsed = components(source);
        let texts = parsed[0]
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            texts,
            vec![
                "do [#answered]",
                "Use staging and run the smoke test.",
                "next task"
            ]
        );
        let ranges = response_row_child_ranges(source);
        assert_eq!(ranges.len(), 4, "{ranges:?}");
        assert!(
            ranges
                .iter()
                .any(|&(start, end)| source[start..end].contains("**staging**"))
        );
        assert!(
            ranges
                .iter()
                .any(|&(start, end)| source[start..end].contains("**production**"))
        );
        assert!(
            ranges
                .iter()
                .all(|&(start, end)| !source[start..end].contains("Use staging"))
        );
    }

    #[test]
    fn canonical_response_back_reference_is_inert() {
        let source = concat!(
            "<!-- agent:queue -->\n",
            "- ~~canonical owner~~ — auto-struck: response: [#r-0123456789ab]\n",
            "  - **Response [#r-0123456789ab]**\n",
            "    - **Timestamp:** 2026-11-01T14:43:00.000Z\n",
            "    - **Summary:** Completed.\n",
            "- ~~another row~~ — auto-struck: response: [#r-0123456789ab]\n",
            "  - **Closed by:** [#r-0123456789ab]\n",
            "- next task\n",
            "<!-- /agent:queue -->\n",
        );

        let parsed = components(source);
        let texts = parsed[0]
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(texts, vec!["canonical owner", "another row", "next task"]);
        assert!(
            response_row_child_ranges(source)
                .iter()
                .any(|&(start, end)| source[start..end].contains("**Closed by:**")),
            "the visible backlink must remain binary-owned inert metadata"
        );
    }

    #[test]
    fn nested_timestamped_response_metadata_is_inert_but_unknown_child_is_typed() {
        let source = concat!(
            "<!-- agent:queue -->\n",
            "- ~~do [#answered]~~ — auto-struck: response: [#r-0123456789ab]\n",
            "  - **Response [#r-0123456789ab]**\n",
            "    - **Timestamp:** 2026-11-01T14:43:00.000Z\n",
            "    - **Summary:** Deployment is blocked. Which target?\n",
            "    - **State:** Unpublished local work remains.\n",
            "    - **Guidance:** Keep this tree until integration.\n",
            "    - **Worktree:** /worktrees/deploy\n",
            "    - **Branch:** fix/deploy\n",
            "    - **Integration:** pending named integration branch\n",
            "    - **Choices:**\n",
            "      - **staging**: Deploy to staging.\n",
            "        - Use staging and run the smoke test.\n",
            "      - **production**: Deploy to production.\n",
            "    - **Next actions:**\n",
            "      - **Action:** Deploy the selected target.\n",
            "- next task\n",
            "<!-- /agent:queue -->\n",
        );

        let parsed = components(source);
        let texts = parsed[0]
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            texts,
            vec![
                "do [#answered]",
                "Use staging and run the smoke test.",
                "next task"
            ]
        );
        let ranges = response_row_child_ranges(source);
        assert_eq!(ranges.len(), 13, "{ranges:?}");
        assert!(ranges.iter().any(|&(start, end)| {
            source[start..end].contains("**Timestamp:** 2026-11-01T14:43:00.000Z")
        }));
        assert!(ranges.iter().any(|&(start, end)| {
            source[start..end].contains("**Summary:** Deployment is blocked")
        }));
        assert!(
            ranges
                .iter()
                .all(|&(start, end)| !source[start..end].contains("Use staging"))
        );
    }

    /// `#responserowqueuehead`: the 2026-11-05 splice. The `**Response**`
    /// wrapper, its timestamp, state and guidance were lost, leaving the
    /// generated `**Summary:**` row orphaned under the operator's first reply.
    /// It must not become a queue item, in any later projected spelling.
    #[test]
    fn orphaned_generated_response_structure_rows_are_never_queue_items() {
        for summary in [
            "      - **Summary:** **Does `preflight --probe` present a search and menu? No.**\n",
            "      - 📥 **Summary:** **Does `preflight --probe` present a search and menu? No.**\n",
            "      - 📥 **Summary:** **Does `preflight --probe` present a search and menu? No.** [#q-e1a0462c7aee]\n",
            "      - ~~**Summary:** **Does `preflight --probe` present a search and menu? No.** [#q-e1a0462c7aee]~~\n",
        ] {
            let source = format!(
                concat!(
                    "<!-- agent:run -->\n",
                    "- ### re [#decisionidstamp] response\n",
                    "  - ~~\"Heads are not stamped.\" should have been a response structure. [#q-219d5d67af28]~~\n",
                    "{}",
                    "        - Operator follow-up under the orphan.\n",
                    "      - **State:** Complete.\n",
                    "      - **Guidance:** Safe to delete this completed row.\n",
                    "      - **Choices:**\n",
                    "      - **Next actions:**\n",
                    "  - Heads are not stamped.\n",
                    "    - 📥 Does preflight --probe present a search + menu? [#q-4fc03236ecf5]\n",
                    "<!-- /agent:run -->\n",
                ),
                summary
            );
            let parsed = components(&source);
            let texts = parsed[0]
                .items
                .iter()
                .map(|item| item.text.as_str())
                .collect::<Vec<_>>();
            assert!(
                texts.iter().all(|text| !text.contains("**Summary:**")
                    && !text.contains("**State:**")
                    && !text.contains("**Guidance:**")
                    && !text.contains("**Choices:**")
                    && !text.contains("**Next actions:**")),
                "{texts:?}\n{source}"
            );
            assert!(
                texts.contains(&"Operator follow-up under the orphan."),
                "an operator child under an orphan stays typed: {texts:?}"
            );
            assert!(
                texts
                    .iter()
                    .any(|text| text.contains("Does preflight --probe present a search + menu?")),
                "{texts:?}"
            );
        }
    }

    #[test]
    fn generated_response_structure_row_predicate_requires_the_reserved_spelling() {
        assert!(is_generated_response_structure_row("**Summary:** Done."));
        assert!(is_generated_response_structure_row(
            "📥 **State:** Complete."
        ));
        assert!(is_generated_response_structure_row(
            "~~**Guidance:** Keep it.~~"
        ));
        assert!(is_generated_response_structure_row(
            "**Timestamp:** 2026-11-05T19:10:10.000Z"
        ));
        assert!(is_generated_response_structure_row("**Choices:**"));
        assert!(is_generated_response_structure_row("**Next actions:**"));
        // Empty values and ordinary operator prose are not the reserved shape.
        assert!(!is_generated_response_structure_row("**Summary:**"));
        assert!(!is_generated_response_structure_row("Summary: please do X"));
        assert!(!is_generated_response_structure_row(
            "Write a **Summary:** section"
        ));
        assert!(!is_generated_response_structure_row(
            "**Choices:** pick one"
        ));
    }

    #[test]
    fn response_row_reserves_every_canonical_id_including_older_retained_responses() {
        let live = concat!(
            "- live parent\n",
            "  - **Response [#r-0123456789ab]:** Summary.\n",
        );
        assert_eq!(response_row_child_ranges(live).len(), 1, "{live}");
        let retained = concat!(
            "- ~~done~~ — auto-struck: response: [#r-fedcba987654]\n",
            "  - **Response [#r-0123456789ab]:** Older retained summary.\n",
        );
        assert_eq!(response_row_child_ranges(retained).len(), 1, "{retained}");

        let malformed = concat!(
            "- ~~done~~ — auto-struck: response: [#r-0123456789ab]\n",
            "  - **Response [#r-NOTCANONICAL]:** Summary.\n",
        );
        assert!(
            response_row_child_ranges(malformed).is_empty(),
            "{malformed}"
        );
    }

    #[test]
    fn generated_next_action_is_inert_but_operator_child_is_typed() {
        let source = concat!(
            "<!-- agent:queue -->\n",
            "- ~~done~~ — auto-struck: response: [#r-0123456789ab]\n",
            "  - **Response [#r-0123456789ab]:** One verification remains.\n",
            "    - **Next actions:**\n",
            "      - **Action:** Open the preview.\n",
            "        - I opened it; verify the screenshot.\n",
            "<!-- /agent:queue -->\n",
        );
        let parsed = components(source);
        let texts = parsed[0]
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(texts, vec!["done", "I opened it; verify the screenshot."]);
        let ranges = response_row_child_ranges(source);
        assert_eq!(ranges.len(), 3, "{ranges:?}");
    }

    #[test]
    fn generated_decision_action_tree_is_inert_but_operator_child_is_typed() {
        let source = concat!(
            "<!-- agent:queue -->\n",
            "- ~~done~~ — auto-struck: response: [#r-0123456789ab]\n",
            "  - **Response [#r-0123456789ab]:** One decision remains.\n",
            "    - **Next actions:**\n",
            "      - **Decision:** Choose the preview target.\n",
            "        - **Action:** Open the selected preview.\n",
            "          - I chose staging; verify the screenshot.\n",
            "<!-- /agent:queue -->\n",
        );
        let parsed = components(source);
        let texts = parsed[0]
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            texts,
            vec!["done", "I chose staging; verify the screenshot."]
        );
        let ranges = response_row_child_ranges(source);
        assert_eq!(ranges.len(), 4, "{ranges:?}");
    }

    #[test]
    fn generated_response_tree_stays_inert_after_lifecycle_metadata_is_appended() {
        let source = concat!(
            "<!-- agent:queue -->\n",
            "- ~~done~~ — auto-struck: response: [#r-0123456789ab]; addressed-at: 2026-11-01T12:00:00Z\n",
            "  - **Response [#r-0123456789ab]:** Choose a target, then publish.\n",
            "    - **Choices:**\n",
            "      - **staging**: Publish to staging.\n",
            "        - Use staging.\n",
            "    - **Next actions:**\n",
            "      - **Action:** Publish the selected target.\n",
            "        - I selected staging; publish it now.\n",
            "    - **Related:** [#q-sibling]\n",
            "      - Compare this result with the sibling.\n",
            "<!-- /agent:queue -->\n",
        );

        let parsed = components(source);
        let texts = parsed[0]
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            texts,
            vec![
                "done",
                "Use staging.",
                "I selected staging; publish it now.",
                "Compare this result with the sibling."
            ]
        );
        let ranges = inert_list_child_ranges(source);
        assert!(
            ranges
                .iter()
                .any(|&(start, end)| source[start..end].contains("**Response"))
        );
        assert!(
            ranges
                .iter()
                .any(|&(start, end)| source[start..end].contains("**Related:**"))
        );
    }

    #[test]
    fn legacy_generated_response_markers_normalize_without_touching_operator_descendants() {
        let source = concat!(
            "<!-- agent:queue -->\n",
            "- parent one\n",
            "  - 📥 **Response [#r-3bb63500aa30]:** First summary.\n",
            "    - 📥 **Choices:**\n",
            "      - 📥 **staging**: Publish to staging.\n",
            "        - 📥 I choose staging; continue.\n",
            "    - 📥 **Next actions:**\n",
            "      - 📥 **Action:** Publish the selected target.\n",
            "        - 📥 Verify the published artifact for me.\n",
            "    - 📥 **Related:** [#q-sibling]\n",
            "      - 📥 Compare the sibling result.\n",
            "- parent two\n",
            "  - 📥 **Response [#r-3bb63500aa30]:** Second summary.\n",
            "- parent three\n",
            "  - 📥 **Response [#r-3bb63500aa30]:** Third summary.\n",
            "- parent four\n",
            "  - 📥 **Response [#r-3bb63500aa30]:** Fourth summary.\n",
            "<!-- /agent:queue -->\n",
        );

        let normalized = normalize_generated_response_lifecycle_markers(source);
        assert_eq!(
            normalized.matches("📥 **Response").count(),
            0,
            "{normalized}"
        );
        assert!(!normalized.contains("📥 **Choices:**"), "{normalized}");
        assert!(!normalized.contains("📥 **Action:**"), "{normalized}");
        assert!(!normalized.contains("📥 **Related:**"), "{normalized}");
        assert!(
            normalized.contains("- 📥 I choose staging; continue."),
            "a genuine operator descendant remains promptable: {normalized}"
        );
        assert!(
            normalized.contains("- 📥 Verify the published artifact for me."),
            "{normalized}"
        );
        assert!(
            normalized.contains("- 📥 Compare the sibling result."),
            "{normalized}"
        );
        assert_eq!(
            normalize_generated_response_lifecycle_markers(&normalized),
            normalized
        );

        let parsed = components(&normalized);
        let texts = parsed[0]
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            texts,
            vec![
                "parent one",
                "📥 I choose staging; continue.",
                "📥 Verify the published artifact for me.",
                "📥 Compare the sibling result.",
                "parent two",
                "parent three",
                "parent four",
            ]
        );
    }

    #[test]
    fn agent_doc_reply_rows_are_inert_and_repair_accidental_lifecycle_markers() {
        let source = concat!(
            "<!-- agent:queue -->\n",
            "- 📥 [#tdunityrunner20261202] Renew the Unity license. <!-- agent-doc:waiting-item -->\n",
            "  - 📥 **Response:** Before 2026-12-02T14:24:21Z, log in to desktop Unity Hub so the cachyos-unity-2022 runner Unity Personal license can be renewed, or provide a self-renewing license (Pro/Plus serial or floating license server) <!-- agent-doc:waiting-metadata -->\n",
            "    - 📥 **Next actions:** <!-- agent-doc:waiting-metadata -->\n",
            "      - 📥 **Action:** Renew the license. <!-- agent-doc:waiting-metadata -->\n",
            "        - 📥 I renewed the license; verify the runner.\n",
            "  - 📥 **Response:** Operator-authored text without the reserved marker.\n",
            "<!-- /agent:queue -->\n",
        );

        let normalized = normalize_generated_response_lifecycle_markers(source);
        assert!(
            normalized.contains("  - **Response:** Before 2026-12-02T14:24:21Z"),
            "{normalized}"
        );
        assert!(
            normalized.contains("    - **Next actions:** <!-- agent-doc:waiting-metadata -->"),
            "{normalized}"
        );
        assert!(
            normalized.contains("      - **Action:** Renew the license."),
            "{normalized}"
        );
        assert!(
            normalized.contains("        - 📥 I renewed the license; verify the runner."),
            "operator-authored descendants remain promptable: {normalized}"
        );
        assert!(
            normalized.contains(
                "  - 📥 **Response:** Operator-authored text without the reserved marker."
            ),
            "an unmarked lookalike remains promptable: {normalized}"
        );

        let parsed = components(&normalized);
        let texts = parsed[0]
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>();
        assert!(
            texts
                .iter()
                .any(|text| text.contains("I renewed the license; verify the runner.")),
            "{texts:#?}"
        );
        assert!(
            texts.iter().any(|text| text
                .contains("**Response:** Operator-authored text without the reserved marker.")),
            "{texts:#?}"
        );
        assert!(
            texts
                .iter()
                .all(|text| !text.contains("Before 2026-12-02T14:24:21Z")),
            "reserved waiting metadata must not become typed work: {texts:#?}"
        );
    }

    #[test]
    fn reply_normalization_migrates_legacy_marker_but_never_claims_glyph_only_rows() {
        let operator_row = "  - ▫️ operator-authored queue work\n";
        assert_eq!(
            normalize_agent_doc_reply_rows(operator_row),
            operator_row,
            "the presentation glyph alone must not establish generated ownership"
        );
        assert!(
            components(&format!(
                "<!-- agent:queue -->\n{operator_row}<!-- /agent:queue -->\n"
            ))[0]
                .items
                .iter()
                .any(|item| item.text.contains("operator-authored queue work")),
            "an operator glyph row remains runnable"
        );

        let legacy = concat!(
            "    - ~~**Response:** choose local <!-- agent-doc:waiting-metadata -->~~\n",
            "      - operator child remains byte-stable\n",
        );
        let normalized = normalize_agent_doc_reply_rows(legacy);
        assert!(
            normalized.contains("    - **Response:** choose local ▫️ <!-- agent-doc:reply -->\n"),
            "{normalized}"
        );
        assert!(
            normalized.contains("      - operator child remains byte-stable\n"),
            "{normalized}"
        );
        assert!(
            !normalized.contains("agent-doc:waiting-metadata"),
            "{normalized}"
        );
        assert_eq!(normalize_agent_doc_reply_rows(&normalized), normalized);
    }

    #[test]
    fn generated_response_marker_repair_preserves_noncanonical_lookalikes() {
        let canonical_details = concat!(
            "- 📥 <details data-agent-doc-response-id=\"r-0123456789ab\">\n",
            "  <summary>Response</summary>\n",
            "  retained response\n",
            "  </details>\n",
        );
        assert_eq!(
            normalize_generated_response_lifecycle_markers(canonical_details),
            canonical_details.replacen("📥 ", "", 1)
        );

        for malformed in [
            "- 📥 **Response [#r-NOTCANONICAL]:** Operator-authored lookalike.\n",
            concat!(
                "- 📥 <details data-agent-doc-response-id=\"r-NOTCANONICAL\">\n",
                "  <summary>Response</summary>\n",
                "  operator-authored lookalike\n",
                "  </details>\n",
            ),
        ] {
            assert_eq!(
                normalize_generated_response_lifecycle_markers(malformed),
                malformed
            );
        }
    }

    #[test]
    fn response_details_inertness_requires_reserved_id_and_balance_not_latest_parent_id() {
        let live = concat!(
            "- live prompt\n",
            "  - <details data-agent-doc-response-id=\"r-0123456789ab\">\n",
            "    <summary>Response</summary>\n",
            "    body\n",
            "    </details>\n",
        );
        assert_eq!(response_details_child_ranges(live).len(), 1, "{live}");
        let retained = concat!(
            "- ~~done~~ — auto-struck: response: [#r-fedcba987654]\n",
            "  - <details data-agent-doc-response-id=\"r-0123456789ab\">\n",
            "    <summary>Response</summary>\n",
            "    older retained response\n",
            "    </details>\n",
        );
        assert_eq!(
            response_details_child_ranges(retained).len(),
            1,
            "{retained}"
        );
        for source in [
            concat!(
                "- ~~done~~\n",
                "  - <details>\n",
                "    <summary>Response</summary>\n",
                "    body\n",
                "    </details>\n",
            ),
            concat!(
                "- ~~done~~\n",
                "  - <details data-agent-doc-response-id=\"r-NOTCANONICAL\">\n",
                "    <summary>Response</summary>\n",
                "    body\n",
                "    </details>\n",
            ),
            concat!(
                "- ~~done~~\n",
                "  - <details data-agent-doc-response-id=\"r-0123456789ab\">\n",
                "    <summary>Response</summary>\n",
                "    body\n",
            ),
        ] {
            assert!(response_details_child_ranges(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn list_comment_recognition_requires_the_whole_leaf_payload() {
        for payload in ["[//]: # (markdown note)", "%%obsidian note%%"] {
            assert!(is_ignored_list_comment(payload), "payload: {payload}");
        }
        for payload in [
            "work then %%obsidian note%%",
            "%%obsidian note%% then work",
            "[//]: # (markdown note) then work",
            "work then [//]: # (markdown note)",
        ] {
            assert!(!is_ignored_list_comment(payload), "payload: {payload}");
        }
    }

    fn queue() -> Component {
        components(DOC)
            .into_iter()
            .find(|c| c.name == "queue")
            .unwrap()
    }

    /// `#queuecruft` / `#queuefreetextsplit`: column-zero prose after a live
    /// free-text list item is a second head, while indented prose remains part
    /// of its list item.
    const CONTINUATION_DOC: &str = "\
<!-- agent:queue -->
- Run Agent Doc on frontend.md error: dispatch-only claude reopen refused
the route-owned host supervisor (pid 1989646) is mapping a STALE binary.
- Indented item first line
  indented second line
- do [#alpha]
  do [#beta]
- ~~struck alone~~
trailing residue
<!-- /agent:queue -->
";

    #[test]
    fn queue_item_spans_bare_heads_and_indented_continuation_lines() {
        let queue = components(CONTINUATION_DOC)
            .into_iter()
            .find(|c| c.name == "queue")
            .unwrap();
        let texts: Vec<&str> = queue.items.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "Run Agent Doc on frontend.md error: dispatch-only claude reopen refused",
                "the route-owned host supervisor (pid 1989646) is mapping a STALE binary.",
                "Indented item first line\n  indented second line",
                "do [#alpha]",
                "struck alone",
            ]
        );
        let first = &queue.items[0];
        assert_eq!(
            &CONTINUATION_DOC[first.start_byte..first.end_byte],
            "- Run Agent Doc on frontend.md error: dispatch-only claude reopen refused\n"
        );
        // The id directive's folded `do [#beta]` and the residue under a line
        // struck on its own are NOT continuation.
        assert!(!queue.items[3].text.contains("beta"));
        assert!(queue.items[4].struck);
        assert!(!queue.items[4].text.contains("residue"));
    }

    /// GH #374: an opener that shares a line with its matching ATX heading is a
    /// component opener here too, exactly as `agent_doc_element::element::parse`
    /// accepts it. Before the fix the overlay required `<!--` to lead the line,
    /// so every respond mutation resolving `queue` through this model reported
    /// "component `queue` was not found" while `read --component queue`,
    /// `patch`, and `lint` all saw the component.
    #[test]
    fn heading_anchored_openers_parse_like_the_element_parser() {
        let doc = concat!(
            "# Session\n\n",
            "## Run <!-- agent:run priority subagents -->\n",
            "- ship it\n",
            "  indented continuation\n",
            "- do [#alpha]\n",
            "<!-- /agent:run -->\n\n",
            "## Loop <!-- agent:loop interval=5m -->\n",
            "- poll ci\n",
            "<!-- /agent:loop -->\n\n",
            "## Backlog <!-- agent:backlog priority -->\n",
            "1. [ ] [#alpha] first task\n",
            "<!-- /agent:backlog -->\n",
        );

        let parsed = components(doc);
        let element = agent_doc_element::element::parse(doc).expect("element parser accepts");
        assert_eq!(
            parsed.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            element.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            "overlay and element parser must resolve the same components"
        );
        assert_eq!(
            parsed.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            vec!["queue", "queue", "backlog"]
        );
        let run = &parsed[0];
        assert_eq!(run.attrs, "priority subagents");
        assert_eq!(
            run.items
                .iter()
                .map(|i| i.text.as_str())
                .collect::<Vec<_>>(),
            vec!["ship it\n  indented continuation", "do [#alpha]"]
        );
        assert!(doc[run.start_byte..].starts_with("## Run <!-- agent:run"));
        assert!(doc[..run.end_byte].ends_with("<!-- /agent:run -->\n"));
        let looped = &parsed[1];
        assert!(looped.attrs.split_whitespace().any(|a| a == "loop"));
        assert_eq!(looped.items[0].text, "poll ci");
        assert_eq!(parsed[2].items[0].id, "alpha");
    }

    /// The heading anchor stays fail-closed: prose that is not an ATX heading,
    /// text after the marker, a closing tag after a heading, another comment
    /// in the heading, and a marker inside inline code or an open quote never
    /// open a component.
    #[test]
    fn non_heading_prefixes_do_not_open_components() {
        for line in [
            "## Run <!-- agent:run --> trailing",
            "## Run <!-- agent:run --> -->",
            "Run <!-- agent:run -->",
            "- ## Run <!-- agent:run -->",
            "####### Run <!-- agent:run -->",
            "##Run <!-- agent:run -->",
            "    ## Run <!-- agent:run -->",
            "\t## Run <!-- agent:run -->",
            "## Run `<!-- agent:run -->`",
            "## Run <!-- /agent:run -->",
            "## Queue: Main <!-- /agent:run -->",
            "## Note <!-- aside --> <!-- agent:run -->",
            "## Say \"hi <!-- agent:run -->",
        ] {
            assert_eq!(parse_open_marker_line(line), None, "{line}");
        }
        assert_eq!(
            parse_open_marker_line("  ### RUN <!-- agent:run go -->"),
            Some(("run".to_string(), "go".to_string()))
        );
        assert_eq!(
            parse_open_marker_line("  <!-- agent:queue go -->  "),
            Some(("queue".to_string(), "go".to_string()))
        );
    }

    /// Any-heading prefix: any ATX heading may label an opener on its own
    /// line, whatever the heading text, for every component kind. The live
    /// 2026-10-26 line `## Queue: Main <!-- agent:run id="main" ... -->` was
    /// refused 363 times by the editor's template-structure guard before this.
    #[test]
    fn any_heading_text_may_prefix_an_opener() {
        for (line, name) in [
            (
                "## Queue: Main <!-- agent:run id=\"main\" subagents preset=\"#spec-test-build-install-commit-push\" priority go -->",
                "run",
            ),
            ("## Notes about runs <!-- agent:run -->", "run"),
            (
                "# Backlog (later) <!-- agent:backlog priority -->",
                "backlog",
            ),
            ("###### `status` view <!-- agent:status -->", "status"),
            ("## Say \"hi\" <!-- agent:todo -->", "todo"),
            ("## <!-- agent:loop interval=5m -->", "loop"),
            ("   #\tTabbed <!-- agent:notes -->\t", "notes"),
        ] {
            assert_eq!(
                parse_open_marker_line(line).map(|(n, _)| n),
                Some(name.to_string()),
                "{line}"
            );
        }
    }

    /// The overlay, the element parser, and the structural gate agree on a
    /// document whose queue opener shares a non-matching heading, and on a
    /// fenced copy of that line (content, never an opener).
    #[test]
    fn non_matching_heading_opener_parses_like_the_element_parser() {
        let doc = concat!(
            "# Session\n\n",
            "## Queue: Main <!-- agent:run id=\"main\" subagents preset=\"#spec-test-build-install-commit-push\" priority go -->\n",
            "- ship it\n",
            "- report:\n",
            "\n",
            "```markdown\n",
            "## Queue: Main <!-- agent:run id=\"main\" -->\n",
            "<!-- /agent:run -->\n",
            "```\n",
            "<!-- /agent:run -->\n\n",
            "## Later items <!-- agent:backlog priority -->\n",
            "1. [ ] [#alpha] first task\n",
            "<!-- /agent:backlog -->\n",
        );
        let element = agent_doc_element::element::parse(doc).expect("element parser accepts");
        assert_eq!(
            agent_doc_element::element::structural_corruption_reason(doc),
            None
        );
        let parsed = components(doc);
        assert_eq!(
            parsed.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            element.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        );
        assert_eq!(
            parsed.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            vec!["queue", "backlog"]
        );
        let run = &parsed[0];
        assert!(run.attrs.contains("id=\"main\""), "{}", run.attrs);
        assert_eq!(run.items[0].text, "ship it");
        assert!(
            run.items
                .iter()
                .all(|item| !item.text.contains("Queue: Main")),
            "heading text is never a queue item: {:?}",
            run.items
        );
        assert!(doc[run.start_byte..].starts_with("## Queue: Main <!-- agent:run"));
        assert!(doc[..run.end_byte].ends_with("```\n<!-- /agent:run -->\n"));
        assert!(
            !element[0]
                .content(doc)
                .contains("## Queue: Main <!-- agent:run id=\"main\" subagents"),
            "the heading text is outside the component body"
        );
    }

    #[test]
    fn loop_and_run_components_project_to_queue_items_after_exact_marker_matching() {
        for (name, attrs) in [("loop", "interval=5m"), ("run", "loop interval=5m")] {
            let doc = format!(
                "<!-- agent:{name} {attrs} -->\n- first\n  continuation\n<!-- /agent:{name} -->\n"
            );
            let parsed = components(&doc);
            assert_eq!(parsed.len(), 1, "{name}");
            assert_eq!(parsed[0].name, "queue", "{name}");
            assert!(
                parsed[0]
                    .attrs
                    .split_whitespace()
                    .any(|attr| attr == "loop")
            );
            assert_eq!(parsed[0].items.len(), 1, "{name}");
            assert_eq!(parsed[0].items[0].text, "first\n  continuation", "{name}");
        }

        let mismatched = "<!-- agent:loop -->\n- first\n<!-- /agent:run -->\n";
        assert_ne!(components(mismatched)[0].end_byte, mismatched.len());
    }

    /// `#componentownclose`: the overlay never lets a loop absorb its sibling
    /// todo's rows, nor rows that follow a foreign close, across the
    /// incident's edit sequence and both opener spellings.
    #[test]
    fn componentownclose_loop_never_absorbs_the_sibling_todo() {
        use agent_doc_element::sibling_span_fixture as fx;
        let todo_texts: Vec<&str> = fx::TODO_ITEMS.lines().collect();
        for style in fx::ALL_STYLES {
            for edit in fx::ALL_EDITS {
                let doc = fx::document(style, edit);
                let parsed = components(&doc);
                let looped = parsed
                    .iter()
                    .find(|c| c.attrs.contains("issues"))
                    .unwrap_or_else(|| panic!("{style:?}/{edit:?}: loop"));
                for item in &looped.items {
                    let line = &doc[item.start_byte..item.end_byte];
                    assert!(
                        !todo_texts
                            .iter()
                            .any(|todo| line.contains(todo.trim_start_matches("- "))),
                        "{style:?}/{edit:?}: loop absorbed a todo row: {line}"
                    );
                    assert!(
                        !line.contains("[/] [#layoutunprovenliveverify]"),
                        "{style:?}/{edit:?}"
                    );
                }
                if fx::edit_keeps_todo(edit) {
                    let todo = parsed
                        .iter()
                        .find(|c| c.name == "todo")
                        .unwrap_or_else(|| panic!("{style:?}/{edit:?}: todo"));
                    let rows: Vec<&str> = todo
                        .items
                        .iter()
                        .map(|item| doc[item.start_byte..item.end_byte].trim_end_matches('\n'))
                        .collect();
                    assert_eq!(rows, todo_texts, "{style:?}/{edit:?}");
                }
                let review = parsed.iter().find(|c| c.name == "review").expect("review");
                assert_eq!(review.items.len(), 1, "{style:?}/{edit:?}");
            }
        }
    }

    /// Rows after a foreign close belong to no component: the dangling loop
    /// stops at `/agent:todo` instead of collecting them.
    #[test]
    fn componentownclose_dangling_component_stops_at_a_foreign_close() {
        let doc = "<!-- agent:loop id=issues -->\n- 📥 Watch\n<!-- /agent:todo -->\n- [ ] [#ahfe] orphaned todo row\n";
        let parsed = components(doc);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].items.len(), 1, "{:?}", parsed[0].items);
        assert_eq!(parsed[0].items[0].text, "📥 Watch");
    }

    #[test]
    fn terminal_tree_hard_wrap_stays_inside_the_diagnostic_item() {
        let source = concat!(
            "<!-- agent:queue -->\n",
            "- In tsift.md session: • Blocked by hook\n",
            "  └ Session persistence continuation found fresh unresolved exchange work\n",
            "for /workspace/tasks/software/tsift.md after the previous cycle was committed.\n",
            "Continue THIS turn in-pane and persist it with agent-doc finalize.\n",
            "- next item\n",
            "<!-- /agent:queue -->\n",
        );
        let queue = components(source)
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        assert_eq!(queue.items.len(), 2, "{:?}", queue.items);
        assert_eq!(
            queue.items[0].text,
            concat!(
                "In tsift.md session: • Blocked by hook\n",
                "  └ Session persistence continuation found fresh unresolved exchange work\n",
                "for /workspace/tasks/software/tsift.md after the previous cycle was committed.\n",
                "Continue THIS turn in-pane and persist it with agent-doc finalize."
            )
        );
        assert_eq!(queue.items[1].text, "next item");
    }

    #[test]
    fn continuation_line_predicate_rejects_queue_native_syntax() {
        for line in [
            "",
            "   ",
            "- item",
            "  - nested",
            "1. ordered",
            "-",
            "`- stray backtick",
            "<!-- /agent:queue -->",
            "```",
            "~~~prompt",
            "---",
            "--- stop",
            "preset #x",
            "dispatch #x",
            "/clear",
            "# Heading",
            "do [#a]",
            "  re #b",
            "[#c] [#d]",
        ] {
            assert!(!queue_continuation_line(line), "{line:?}");
        }
        for line in [
            "  indented prose",
            "  /project/path is a path, not a command",
            "  #tag is a tag, not a heading",
            "  > quoted log",
        ] {
            assert!(queue_continuation_line(line), "{line:?}");
        }
    }

    #[test]
    fn split_struck_body_handles_annotated_and_bare_and_unstruck() {
        // Bare strike wrapper.
        assert_eq!(split_struck_body("~~foo~~"), (true, "foo"));
        // Annotated wrapper (#qstrikenote): note lives outside the closing `~~`.
        assert_eq!(
            split_struck_body("~~foo bar~~ — auto-struck: answered this cycle (#ftstrike)"),
            (true, "foo bar")
        );
        // Not struck.
        assert_eq!(split_struck_body("foo"), (false, "foo"));
        // Empty wrapper is not a real strike.
        assert_eq!(split_struck_body("~~~~"), (false, "~~~~"));
    }

    #[test]
    fn annotated_struck_free_text_head_parses_as_struck() {
        let doc = "\
<!-- agent:queue -->
- ~~answered free-text head~~ — auto-struck: answered this cycle (#ftstrike)
<!-- /agent:queue -->
";
        let q = components(doc)
            .into_iter()
            .find(|c| c.name == "queue")
            .unwrap();
        assert_eq!(q.items.len(), 1);
        assert!(q.items[0].struck, "annotated head stays struck");
        assert_eq!(q.items[0].text, "answered free-text head");
        assert_eq!(q.items[0].kind, ItemKind::FreeText);
    }

    #[test]
    fn queue_navigation_heading_is_presentation_not_identity_or_lifecycle() {
        let doc = "\
<!-- agent:queue -->
- ### do [#active]
- ### ~~🚧 do [#done]~~
- ~~### do [#legacy]~~
<!-- /agent:queue -->
";
        let q = components(doc)
            .into_iter()
            .find(|c| c.name == "queue")
            .unwrap();

        assert_eq!(q.items[0].text, "do [#active]");
        assert_eq!(q.items[0].id, "active");
        assert!(!q.items[0].struck);
        assert_eq!(q.items[1].text, "🚧 do [#done]");
        assert_eq!(q.items[1].id, "done");
        assert!(q.items[1].struck);
        assert_eq!(q.items[2].text, "do [#legacy]");
        assert_eq!(q.items[2].id, "legacy");
        assert!(q.items[2].struck);
    }

    #[test]
    fn parses_components_and_attrs() {
        let comps = components(DOC);
        let names: Vec<&str> = comps.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["queue", "backlog"]);
        assert_eq!(comps[0].attrs, "priority go");
    }

    #[test]
    fn typed_items_with_ids_pins_and_strike() {
        let q = queue();
        assert_eq!(q.items.len(), 4);

        let alpha = &q.items[0];
        assert_eq!(alpha.id, "alpha");
        assert_eq!(alpha.kind, ItemKind::Do);
        assert!(alpha.pinned);
        assert!(!alpha.struck);

        let beta = &q.items[1];
        assert_eq!(beta.id, "beta");
        assert!(beta.struck, "~~…~~ marks the node struck");
        assert!(beta.pinned, "pin survives inside a strikethrough wrapper");
        assert_eq!(beta.kind, ItemKind::Do);

        let free = &q.items[2];
        assert_eq!(free.kind, ItemKind::FreeText);
        assert!(free.pinned);
        assert!(
            free.id.starts_with("ft-"),
            "free text gets a content id: {}",
            free.id
        );
        assert_eq!(free.text, "Free-text bug report about pins");

        let gamma = &q.items[3];
        assert_eq!(gamma.kind, ItemKind::Reference);
        assert_eq!(gamma.id, "gamma");
    }

    /// `#queuerowworkidentity`: a row's node id is its work identity, never
    /// the attempt projection's first `[#...]` token. A claimed row, every
    /// lifecycle re-projection of it, and its bare or struck copy are one node.
    #[test]
    fn queuerowworkidentity_claimed_projected_and_struck_rows_share_one_id() {
        let id_rows = [
            "do [#foo]",
            "~~do [#foo]~~",
            "~~do [#foo]~~ — auto-struck: answered",
            "☑️ [#subagent:x#1] do [#foo]",
            "🔄 [#subagent:x#1] do [#foo]",
            "📥 [#subagent:x#2] do [#foo]",
            "❔ do [#foo]",
            "🙋☑️ [#subagent:x#1] do [#foo]",
            "☑️ ⟦subagent:x#1⟧ do [#foo]",
            "🟡 ⟦owner=subagent:x;generation=4⟧ do [#foo]",
            ":pushpin: ☑️ [#subagent:x#1] do [#foo]",
        ];
        for row in id_rows {
            let item = parse_item(row, 0, row.len(), true);
            assert_eq!(item.id, "foo", "{row:?}");
            assert!(item.explicit_id, "{row:?}");
            assert_eq!(item.work_text(), "do [#foo]", "{row:?}");
        }

        let bare = parse_item("fix the parser", 0, 0, true);
        for row in [
            "~~fix the parser~~",
            "☑️ [#subagent:x#1] fix the parser",
            "🔄 [#subagent:x#1] fix the parser",
            "📥 fix the parser",
        ] {
            let item = parse_item(row, 0, row.len(), true);
            assert_eq!(item.id, bare.id, "{row:?}");
            // The owner tag is projection metadata, not a tracked-work id that
            // could be offered as a `--done` target.
            assert!(!item.explicit_id, "{row:?}");
        }

        // Text and surface flags are untouched: only the identity moved.
        let claimed = parse_item("☑️ [#subagent:x#1] do [#foo]", 0, 0, true);
        assert_eq!(claimed.text, "☑️ [#subagent:x#1] do [#foo]");
        assert_eq!(claimed.kind, ItemKind::FreeText);
    }

    /// `#queuerowworkidentity` compatibility: every row WITHOUT a leading
    /// attempt projection keys exactly as the pre-change overlay did (first
    /// `[#...]` token, else a content hash of the full text), so documents,
    /// node keys, and replicas built from such rows are unchanged. The
    /// canonical operator-answer row keeps its request id.
    #[test]
    fn queuerowworkidentity_unprojected_rows_keep_their_previous_ids() {
        fn previous_id(row: &str) -> String {
            let item = parse_item(row, 0, row.len(), true);
            extract_bracket_id(&item.text)
                .unwrap_or_else(|| content_item_id(&item.kind, &item.text))
        }
        for row in [
            "do [#alpha]",
            "re [#gamma]",
            ":pushpin: do [#beta]",
            "~~:pushpin: do [#beta]~~",
            "[#bare]",
            "do #hashonly",
            "Free-text bug report about pins",
            "fix [#inline] handling later",
            "[ ] [#task] backlog task",
            "🙋 [#subagent-operator-request-abc]: yes, ship it",
            "🙋 [#legacy-request] old prefix form",
        ] {
            assert_eq!(
                parse_item(row, 0, row.len(), true).id,
                previous_id(row),
                "{row:?}"
            );
        }
        assert_eq!(
            parse_item("🙋 [#subagent-operator-request-abc]: yes", 0, 0, true).id,
            "subagent-operator-request-abc"
        );
    }

    /// `#queuerowworkidentity`: multiline prompts key on their work too.
    #[test]
    fn queuerowworkidentity_multiline_prompt_ignores_leading_projection() {
        let bare = parse_multiline_item("x", 0, 0, 0, "title\nbody".to_string(), false);
        let claimed = parse_multiline_item(
            "x",
            0,
            0,
            0,
            "☑️ [#subagent:x#1] title\nbody".to_string(),
            false,
        );
        assert_eq!(claimed.id, bare.id);
        assert!(!claimed.explicit_id);
        let id_head = parse_multiline_item(
            "x",
            0,
            0,
            0,
            "🔄 [#subagent:x#1] do [#foo]\nbody".to_string(),
            false,
        );
        assert_eq!(id_head.id, "foo");
    }

    #[test]
    fn pin_and_strike_are_node_attributes_not_text() {
        // The whole point of the overlay: identity + flags are structural, so a
        // pinned and an unpinned spelling of the same item share an id.
        let pinned = parse_item(":pushpin: do [#x]", 0, 0, true);
        let bare = parse_item("do [#x]", 0, 0, true);
        assert_eq!(pinned.id, bare.id);
        assert!(pinned.pinned && !bare.pinned);
        assert_eq!(pinned.text, bare.text);
    }

    #[test]
    fn backlog_checkbox_state() {
        let b = components(DOC)
            .into_iter()
            .find(|c| c.name == "backlog")
            .unwrap();
        assert_eq!(b.items.len(), 2);
        assert_eq!(b.items[0].kind, ItemKind::BacklogTask { checkbox: ' ' });
        assert_eq!(b.items[0].id, "alpha");
        assert_eq!(b.items[1].kind, ItemKind::BacklogTask { checkbox: 'x' });
    }

    #[test]
    fn end_byte_spans_full_component_for_both_close_spellings() {
        // #gszq: the close marker must be recognized for BOTH spellings so
        // `Component.end_byte` spans the whole component (through the close
        // line) rather than collapsing to just past the open marker.
        for src in [
            // standard slash-then-`agent:name`
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
            // legacy `agent:`-then-slash
            "<!-- agent:queue -->\n- do [#a]\n<!-- agent:/queue -->\n",
        ] {
            let comps = components(src);
            assert_eq!(comps.len(), 1, "src={src:?}");
            let q = &comps[0];
            // end_byte must reach the end of the close-marker line, i.e. the
            // full source length here, not the offset just past the open line.
            assert_eq!(
                q.end_byte,
                src.len(),
                "end_byte must span the full component (src={src:?})"
            );
            assert!(
                q.end_byte > q.start_byte + "<!-- agent:queue -->\n".len(),
                "end_byte must extend past the open marker (src={src:?})"
            );
            // byte-span slicing yields the whole component, not a truncated head.
            let slice = &src[q.start_byte..q.end_byte];
            assert!(slice.contains("[#a]"), "span must include the item");
            assert!(
                slice.contains("queue -->") && slice.matches("-->").count() == 2,
                "span must include the close marker (src={src:?})"
            );
        }
    }

    #[test]
    fn markers_inside_code_fence_are_ignored() {
        let src = "\
<!-- agent:queue -->
- do [#real]
<!-- /agent:queue -->

```markdown
<!-- agent:queue -->
- do [#fake]
<!-- /agent:queue -->
```
";
        let comps = components(src);
        assert_eq!(comps.len(), 1, "fenced markers must not open a component");
        assert_eq!(comps[0].items.len(), 1);
        assert_eq!(comps[0].items[0].id, "real");
    }

    #[test]
    fn a_choices_label_that_drifted_under_the_summary_stays_inert() {
        // `#waitingpacketowner`: Choices one level too deep (beneath the
        // response's Summary) are still generated metadata, so no offered
        // option becomes an executable queue leaf.
        let source = concat!(
            "- tmstorm [#q-569d77146187]\n",
            "  - **Response [#r-30853087f858]**\n",
            "    - **Timestamp:** 2026-11-02T00:20:07.000Z\n",
            "    - **Summary:** The draft is ready, not sent.\n",
            "      - **Choices:**\n",
            "        - **UI role**: create a studio role and assign it.\n",
            "        - **Tester only**: no admin role.\n",
            "- an ordinary queue row\n",
        );
        let ranges = inert_list_child_ranges(source);
        let inert = |needle: &str| {
            let at = source.find(needle).unwrap();
            ranges.iter().any(|&(start, end)| at >= start && at < end)
        };
        assert!(inert("**Choices:**"));
        assert!(inert("**UI role**"));
        assert!(inert("**Tester only**"));
        assert!(!inert("an ordinary queue row"));
        assert!(!inert("tmstorm"));
    }
}
