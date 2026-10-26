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
fn split_struck_body(body: &str) -> (bool, &str) {
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
    pub fn close_at(&self, line_start: usize, line: &str) -> Option<String> {
        if self.in_code(line_start) {
            return None;
        }
        parse_close_marker(line.trim())
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
    let item = parse_item(item_content, 0, item_content.len());
    !item.struck
        && item.kind == ItemKind::FreeText
        && !item.text.is_empty()
        && !item.text.starts_with("[#")
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
    match extract_bracket_id(work) {
        Some(id) => (id, true),
        None => (content_item_id(&classify(work), work), false),
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

fn parse_item(raw_line_content: &str, start_byte: usize, end_byte: usize) -> Item {
    let raw = raw_line_content.trim().to_string();
    let mut body = raw.as_str();

    // Strikethrough wrapper. Tolerates an `#qstrikenote` annotation appended
    // outside the `~~…~~` wrapper (`~~text~~ — auto-struck: …`): the line is still
    // a struck node whose text is the inner content.
    let (struck, inner) = split_struck_body(body);
    if struck {
        body = inner;
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
    let text = body.trim().to_string();
    let (id, explicit_id) = work_item_id(&text);
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

    let mut offset = 0usize;
    // `#queuecruft`: byte offset where the open queue list item's content
    // starts, while that item still accepts continuation lines.
    let mut continuable_item: Option<(usize, QueueContinuationState)> = None;
    for line in source.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        if markers.in_code(line_start) {
            continuable_item = None;
            continue;
        }
        let content = line.trim_end_matches('\n');
        let content = content.strip_suffix('\r').unwrap_or(content);
        let trimmed = content.trim();

        if let Some((raw_start, state)) = continuable_item.as_mut() {
            if state.accepts_line(content)
                && let Some(comp) = open.as_mut()
                && let Some(last) = comp.items.last_mut()
            {
                let joined = &source[*raw_start..line_start + content.len()];
                *last = parse_item(joined, last.start_byte, offset);
                continue;
            }
            continuable_item = None;
        }

        if let Some(name) = markers.close_at(line_start, trimmed) {
            if let Some(mut comp) = open.take() {
                if comp.name == name {
                    comp.end_byte = offset;
                    project_runnable_component(&mut comp);
                    include_queue_multiline_items(source, &mut comp, line_start, &ranges);
                    out.push(comp);
                    continue;
                }
                // Mismatched close — restore and fall through.
                open = Some(comp);
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
            comp.items
                .push(parse_item(item_content, line_start, offset));
            if is_runnable_component(&comp.name) && queue_item_accepts_continuation(item_content) {
                // `item_content` is a suffix slice of `content`, which starts at
                // `line_start`.
                continuable_item = Some((
                    line_start + (content.len() - item_content.len()),
                    QueueContinuationState::default(),
                ));
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
            let item = parse_item(row, 0, row.len());
            assert_eq!(item.id, "foo", "{row:?}");
            assert!(item.explicit_id, "{row:?}");
            assert_eq!(item.work_text(), "do [#foo]", "{row:?}");
        }

        let bare = parse_item("fix the parser", 0, 0);
        for row in [
            "~~fix the parser~~",
            "☑️ [#subagent:x#1] fix the parser",
            "🔄 [#subagent:x#1] fix the parser",
            "📥 fix the parser",
        ] {
            let item = parse_item(row, 0, row.len());
            assert_eq!(item.id, bare.id, "{row:?}");
            // The owner tag is projection metadata, not a tracked-work id that
            // could be offered as a `--done` target.
            assert!(!item.explicit_id, "{row:?}");
        }

        // Text and surface flags are untouched: only the identity moved.
        let claimed = parse_item("☑️ [#subagent:x#1] do [#foo]", 0, 0);
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
            let item = parse_item(row, 0, row.len());
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
                parse_item(row, 0, row.len()).id,
                previous_id(row),
                "{row:?}"
            );
        }
        assert_eq!(
            parse_item("🙋 [#subagent-operator-request-abc]: yes", 0, 0).id,
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
        let pinned = parse_item(":pushpin: do [#x]", 0, 0);
        let bare = parse_item("do [#x]", 0, 0);
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
}
