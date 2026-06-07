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

/// A single component item as a typed node: stable id, byte span, surface flags,
/// and kind. `text` is the content with strike/pin markers removed; `raw` keeps
/// the original line content (after the `- ` / `N.` bullet).
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

/// Parse a `<!-- /agent:name -->` close marker, returning the name.
fn parse_close_marker(trimmed: &str) -> Option<String> {
    let inner = trimmed.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    let rest = inner
        .strip_prefix("agent:/")
        .or_else(|| inner.strip_prefix("agent:")?.strip_prefix('/'))?;
    let name = rest.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Strip a leading list bullet (`- `, `* `, `N. `) returning the item content.
fn strip_bullet(line: &str) -> Option<&str> {
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

    // Strikethrough wrapper.
    let struck = body.starts_with("~~") && body.ends_with("~~") && body.len() >= 4;
    if struck {
        body = body[2..body.len() - 2].trim();
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
    }
}

/// Parse all agent-components in `source` into typed nodes, in document order.
/// Markers and list lines inside fenced/indented code are ignored.
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
            if let Some(comp) = open.take() {
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
    if let Some(comp) = open.take() {
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
