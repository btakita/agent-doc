//! Structured markdown block AST for agent-doc session documents.
//!
//! Phase 1 of `#md-ast-document-model` (`tasks/agent-doc/plan-md-ast-document-model.md`):
//! a `tree-sitter-md`-based block tree with byte spans and stable, content-derived
//! ids. Pure and standalone — **no agent-doc coupling**. Later phases layer the
//! `<!-- agent:name -->` component overlay (queue/backlog/exchange items as typed
//! nodes), a structured CRDT, and node-keyed mutations on top.
//!
//! Why this exists: agent-doc has mutated the document as *text*, so queue-consume,
//! dedup, and IPC patches act on what a line *looks like* rather than *which item*
//! it is — the root cause of the queue/dedup/live-buffer data-loss family. A block
//! tree with stable ids is the substrate that lets later phases key those
//! operations on node identity instead of text.

pub mod crdt;
pub mod events;
pub mod exchange_tree;
pub mod mutations;
pub mod overlay;
pub mod reply_row;
pub mod tree_status_row;

use std::hash::{Hash, Hasher};

/// Coarse block-level classification, mapped from `tree-sitter-md` node kinds.
/// Unmapped kinds are preserved verbatim in [`BlockKind::Other`] so the tree is
/// lossless even as the grammar evolves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    Document,
    Section,
    Heading,
    List,
    ListItem,
    FencedCode,
    IndentedCode,
    HtmlBlock,
    Paragraph,
    BlockQuote,
    ThematicBreak,
    Other(String),
}

impl BlockKind {
    fn from_node_kind(kind: &str) -> Self {
        match kind {
            "document" => BlockKind::Document,
            "section" => BlockKind::Section,
            "atx_heading" | "setext_heading" => BlockKind::Heading,
            "list" => BlockKind::List,
            "list_item" => BlockKind::ListItem,
            "fenced_code_block" => BlockKind::FencedCode,
            "indented_code_block" => BlockKind::IndentedCode,
            "html_block" => BlockKind::HtmlBlock,
            "paragraph" => BlockKind::Paragraph,
            "block_quote" => BlockKind::BlockQuote,
            "thematic_break" => BlockKind::ThematicBreak,
            other => BlockKind::Other(other.to_string()),
        }
    }
}

/// A node in the markdown block tree: its kind, byte span into the source, a
/// stable content-derived id, and its block children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    /// Inclusive start byte offset into the parsed source.
    pub start_byte: usize,
    /// Exclusive end byte offset into the parsed source.
    pub end_byte: usize,
    /// Stable id derived from `(kind, trimmed source text)`. Phase 1 keys identity
    /// on content; later phases reuse durable `[#hash]` ids + provenance so a node
    /// keeps its id across edits.
    pub id: String,
    pub children: Vec<Block>,
}

impl Block {
    /// The source slice this block spans.
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        source.get(self.start_byte..self.end_byte).unwrap_or("")
    }

    /// Depth-first iteration helper: push this block's kind and all descendants'.
    pub fn collect_kinds(&self, out: &mut Vec<BlockKind>) {
        out.push(self.kind.clone());
        for child in &self.children {
            child.collect_kinds(out);
        }
    }
}

/// Derive a short, deterministic id from a block's kind and trimmed source text.
/// `DefaultHasher` (fixed-key SipHash) is stable across processes, which is what
/// makes the same content yield the same id between cycles.
fn block_id(kind: &BlockKind, text: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{kind:?}").hash(&mut hasher);
    text.trim().hash(&mut hasher);
    format!("{:08x}", hasher.finish() & 0xffff_ffff)
}

/// Parse markdown `source` into a block tree, returning the `Document` root.
/// Returns `None` only if the tree-sitter language fails to load or the parser
/// returns no tree (both effectively impossible for valid in-process setup).
pub fn parse(source: &str) -> Option<Block> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_md::LANGUAGE.into()).ok()?;
    let tree = parser.parse(source, None)?;
    Some(build_block(tree.root_node(), source.as_bytes()))
}

/// Return byte ranges occupied by Markdown inline-code spans.
///
/// The block grammar deliberately does not interpret inline syntax. Queue and
/// component overlays that recognize executable list-shaped text need this
/// second AST projection so a literal `` `- text` `` string cannot become a
/// list item merely because its payload starts with a Markdown bullet.
pub fn inline_code_ranges(source: &str) -> Vec<std::ops::Range<usize>> {
    fn collect(node: tree_sitter::Node<'_>, out: &mut Vec<std::ops::Range<usize>>) {
        if node.kind() == "code_span" {
            out.push(node.start_byte()..node.end_byte());
            return;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect(child, out);
        }
    }

    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_md::INLINE_LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    collect(tree.root_node(), &mut ranges);
    ranges
}

/// [`inline_code_ranges`], with list-item boundaries respected.
///
/// The inline grammar sees `source` as one paragraph, so an unbalanced
/// backtick inside one list item pairs with a backtick in a later sibling item
/// and every list marker between them reads as inline code
/// (`#operatorrepliesnested`: a truncated quote in one waiting item's text
/// turned the next two waiting items, and the operator replies beneath them,
/// into inert text, and the Waiting-on-you refresh regenerated them without
/// those replies). CommonMark bounds a code span by its containing block: a
/// span opened on a list-item line never extends past the start of the next
/// list-item line. Such an opener is a literal backtick and scanning resumes
/// right after it. A span opened on a line that is not a list item keeps the
/// one-paragraph reading, so a multiline quoted string whose later line is
/// list-shaped stays inline code.
///
/// `item_marker` returns the byte offset of the list marker within a line
/// (the line has no terminator) when that line is list-item-shaped.
pub fn inline_code_ranges_within_list_items(
    source: &str,
    item_marker: impl Fn(&str) -> Option<usize>,
) -> Vec<std::ops::Range<usize>> {
    // (line start, absolute marker byte) of every list-item-shaped line.
    let mut items: Vec<(usize, usize)> = Vec::new();
    let mut line_bounds: Vec<(usize, usize)> = Vec::new();
    let mut pos = 0usize;
    for raw in source.split_inclusive('\n') {
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        let line = line.strip_suffix('\r').unwrap_or(line);
        line_bounds.push((pos, pos + raw.len()));
        if let Some(marker) = item_marker(line) {
            items.push((pos, pos + marker));
        }
        pos += raw.len();
    }
    let line_start_of = |byte: usize| -> usize {
        let index = line_bounds.partition_point(|&(_, end)| end <= byte);
        line_bounds
            .get(index)
            .map_or(source.len(), |&(start, _)| start)
    };

    let mut accepted: Vec<std::ops::Range<usize>> = Vec::new();
    let mut from = 0usize;
    'parse: while from < source.len() {
        let mut ranges = inline_code_ranges(&source[from..]);
        ranges.sort_by_key(|range| range.start);
        for range in ranges {
            let range = (range.start + from)..(range.end + from);
            let opener_line = line_start_of(range.start);
            let item = items.binary_search_by_key(&opener_line, |&(start, _)| start);
            if let Ok(item) = item {
                let (_, marker) = items[item];
                let is_item = !accepted.iter().any(|done| done.contains(&marker));
                let next_item = items.get(item + 1).map(|&(start, _)| start);
                if is_item && next_item.is_some_and(|next| range.end > next) {
                    let run = source[range.start..]
                        .bytes()
                        .take_while(|byte| *byte == b'`')
                        .count()
                        .max(1);
                    from = range.start + run;
                    continue 'parse;
                }
            }
            accepted.push(range);
        }
        break;
    }
    accepted
}

fn build_block(node: tree_sitter::Node<'_>, src: &[u8]) -> Block {
    let kind = BlockKind::from_node_kind(node.kind());
    let start_byte = node.start_byte();
    let end_byte = node.end_byte();
    let text = std::str::from_utf8(src.get(start_byte..end_byte).unwrap_or(&[])).unwrap_or("");
    let id = block_id(&kind, text);
    let mut children = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        children.push(build_block(child, src));
    }
    Block {
        kind,
        start_byte,
        end_byte,
        id,
        children,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<BlockKind> {
        let doc = parse(source).expect("parse");
        let mut out = Vec::new();
        doc.collect_kinds(&mut out);
        out
    }

    #[test]
    fn root_is_document() {
        let doc = parse("# Title\n").expect("parse");
        assert_eq!(doc.kind, BlockKind::Document);
        assert_eq!(doc.start_byte, 0);
    }

    #[test]
    fn parses_headings_lists_and_code() {
        let k = kinds("# Title\n\n- item one\n- item two\n\n```rust\nfn x() {}\n```\n");
        assert!(k.contains(&BlockKind::Heading), "headings: {k:?}");
        assert!(k.contains(&BlockKind::List), "lists: {k:?}");
        assert!(k.contains(&BlockKind::ListItem), "list items: {k:?}");
        assert!(k.contains(&BlockKind::FencedCode), "fenced code: {k:?}");
    }

    #[test]
    fn ids_are_stable_and_content_derived() {
        let a = parse("# Hello\n").unwrap();
        let b = parse("# Hello\n").unwrap();
        assert_eq!(a.id, b.id, "same content yields same id");
        let c = parse("# Goodbye\n").unwrap();
        assert_ne!(a.id, c.id, "different content yields different id");
    }

    #[test]
    fn block_text_round_trips_the_span() {
        let src = "# Title\n\nA paragraph.\n";
        let doc = parse(src).unwrap();
        // The document span covers the whole source.
        assert_eq!(doc.text(src), src);
    }

    fn item_marker(line: &str) -> Option<usize> {
        let indent = line.len() - line.trim_start_matches(' ').len();
        line[indent..].starts_with("- ").then_some(indent)
    }

    /// `#operatorrepliesnested`: an unbalanced backtick in one list item must
    /// not pair with a backtick in a later sibling and hide the list markers
    /// between them as inline code.
    #[test]
    fn a_code_span_opened_in_a_list_item_ends_before_the_next_item() {
        let source = concat!(
            "  - quote `a` and `- ### truncated\n",
            "  - [#second] another item\n",
            "    - Why do you recommend it?\n",
            "  - [#third] closes ` and `b` here\n",
        );
        let ranges = inline_code_ranges_within_list_items(source, item_marker);
        for needle in ["- [#second]", "- Why do you", "- [#third]"] {
            let byte = source.find(needle).unwrap();
            assert!(
                !ranges.iter().any(|range| range.contains(&byte)),
                "{needle:?} is a list marker, not inline code: {ranges:?}"
            );
        }
        // The balanced spans on each line are still inline code.
        for needle in ["`a`", "`b`"] {
            let byte = source.find(needle).unwrap();
            assert!(ranges.iter().any(|range| range.contains(&byte)), "{needle}");
        }
        // The unbounded grammar is what pairs them across items.
        let unbounded = inline_code_ranges(source);
        let second = source.find("- [#second]").unwrap();
        assert!(unbounded.iter().any(|range| range.contains(&second)));
    }

    #[test]
    fn a_code_span_opened_outside_a_list_item_still_spans_list_shaped_lines() {
        let source = "`quoted status\n- ### quoted line\nstill quoted`\n- run tests\n";
        let ranges = inline_code_ranges_within_list_items(source, item_marker);
        let quoted = source.find("- ### quoted").unwrap();
        let run = source.find("- run tests").unwrap();
        assert!(ranges.iter().any(|range| range.contains(&quoted)));
        assert!(!ranges.iter().any(|range| range.contains(&run)));
        // A span closed on its own item line, or on a continuation line before
        // the next item, stays inline code.
        let source = "- see `- ### one\n  two` here\n- next `x`\n";
        let ranges = inline_code_ranges_within_list_items(source, item_marker);
        let inner = source.find("- ### one").unwrap();
        let next = source.find("- next").unwrap();
        assert!(ranges.iter().any(|range| range.contains(&inner)));
        assert!(!ranges.iter().any(|range| range.contains(&next)));
    }

    #[test]
    fn inline_code_ranges_cover_list_shaped_string_payloads() {
        let source = "before `- ### quoted status` after\n`multi-line\n- string`\n";
        let ranges = inline_code_ranges(source);

        for needle in ["- ### quoted", "- string"] {
            let byte = source.find(needle).unwrap();
            assert!(
                ranges.iter().any(|range| range.contains(&byte)),
                "{needle:?} must be covered by an inline-code AST range: {ranges:?}"
            );
        }
    }
}
