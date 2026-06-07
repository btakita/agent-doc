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
pub mod overlay;

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
    parser
        .set_language(&tree_sitter_md::LANGUAGE.into())
        .ok()?;
    let tree = parser.parse(source, None)?;
    Some(build_block(tree.root_node(), source.as_bytes()))
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
}
