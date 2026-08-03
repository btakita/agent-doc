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

/// Parse `<!-- agent:name -->` open markers, returning `(name, attrs)`.
fn parse_open_marker(trimmed: &str) -> Option<(String, String)> {
    let inner = trimmed.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
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

fn strip_one_pin(text: &str) -> Option<(&str, bool)> {
    let t = text.trim_start();
    for m in OPERATOR_PIN_MARKERS {
        if let Some(rest) = t.strip_prefix(m) {
            return Some((rest.trim_start(), true));
        }
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

fn item_id(kind: &ItemKind, text: &str) -> String {
    if let Some(id) = extract_bracket_id(text) {
        return id;
    }
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
    let id = item_id(&kind, &text);
    Item {
        id,
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
    let id = item_id(&kind, &text);
    Item {
        id,
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
) -> Vec<Item> {
    let lines = source_lines(source, body_start, body_end);
    let mut items = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.content.trim();
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
            index += 1;
            continue;
        };
        let close_index = (index + 1..lines.len()).find(|candidate| {
            let candidate_line = lines[*candidate];
            candidate_line.content.trim() == closer
                && (closer != "---" || !in_code(code_ranges, candidate_line.start_byte))
        });
        let Some(close_index) = close_index else {
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
    let multiline = queue_multiline_items(source, body_start, body_end, code_ranges);
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
    let ranges = code_ranges(source);
    let mut out: Vec<Component> = Vec::new();
    let mut open: Option<Component> = None;

    let mut offset = 0usize;
    for line in source.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        if in_code(&ranges, line_start) {
            continue;
        }
        let content = line.trim_end_matches('\n');
        let trimmed = content.trim();

        if let Some(name) = parse_close_marker(trimmed) {
            if let Some(mut comp) = open.take() {
                if comp.name == name {
                    comp.end_byte = offset;
                    include_queue_multiline_items(source, &mut comp, line_start, &ranges);
                    out.push(comp);
                    continue;
                }
                // Mismatched close — restore and fall through.
                open = Some(comp);
            }
            continue;
        }
        if let Some((name, attrs)) = parse_open_marker(trimmed) {
            // A new open marker closes any dangling component implicitly.
            if let Some(mut comp) = open.take() {
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
        }
    }
    if let Some(mut comp) = open.take() {
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
