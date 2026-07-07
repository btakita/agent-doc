//! Phase 0 lossless-tree adapter for agent-doc session documents (`#lzlosstree`).
//!
//! Builds a lazily [`LosslessTreeCrdt`] whose leaves own **every byte** of a
//! session document, so the defining invariant holds by construction:
//!
//! ```text
//! render(parse(doc)) == doc
//! ```
//!
//! for current session docs, compacted docs, and corrupt / invalid intermediate
//! editor buffers alike. This is the **shadow projection** the rollout plan's
//! Phase 0/1 call for: the flat text CRDT stays the authority; this tree is built,
//! rendered, and equality-checked (see [`shadow_audit`]) but never used for
//! closeout decisions.
//!
//! ## Why it is lossless by construction
//!
//! Rather than trust a semantic parser to reproduce the source, [`parse`] **tiles**
//! `0..doc.len()` with leaves that each hold an exact source substring, in offset
//! order. Semantic structure is layered on *only where the overlay parser is
//! confident* (`<!-- agent:name -->` components become [`ElementNode`]s, their
//! markers [`Token`](LeafKind::Token) leaves, blank-line gaps
//! [`Trivia`](LeafKind::Trivia)); everything else — prose, unknown, or invalid
//! spans — is a [`Raw`](LeafKind::Raw) leaf. Because the emitted leaves partition
//! the byte range, their in-order concatenation is the source, whatever the parser
//! did. A semantic AST that dropped or normalized bytes could not pass this.

use agent_doc_markdown_ast::overlay::{Component, components};
use lazily::{LeafKind, LosslessTreeCrdt, NodeSeed, TreeNodeId};

fn leaf(kind: LeafKind, text: &str) -> NodeSeed {
    NodeSeed::Leaf {
        kind,
        text: text.to_string(),
    }
}

fn element(kind: &str) -> NodeSeed {
    NodeSeed::Element {
        kind: kind.to_string(),
    }
}

/// Append `seed` as the last child of `parent` (after `after`), returning its id.
/// `parent` is always live here (root or a just-created element), so create can
/// only fail on a logic bug — surface it loudly rather than drop bytes silently.
fn append(
    tree: &mut LosslessTreeCrdt,
    parent: TreeNodeId,
    after: Option<TreeNodeId>,
    seed: NodeSeed,
) -> TreeNodeId {
    tree.create_node(parent, after, seed)
        .expect("create_node under a live parent")
}

/// Parse a session document into a lossless tree whose rendered text is byte-for-
/// byte the input. See the module docs for the tiling invariant.
pub fn parse(doc: &str) -> LosslessTreeCrdt {
    let mut tree = LosslessTreeCrdt::new(1);
    let root = TreeNodeId::ROOT;
    let mut prev: Option<TreeNodeId> = None;

    let mut comps = components(doc);
    comps.sort_by_key(|c| c.start_byte);

    let len = doc.len();
    let mut cursor = 0usize;
    for comp in &comps {
        // Skip overlapping / nested / invalid spans so the tiling stays a strict
        // partition (a nested component's bytes are already inside its parent's
        // Raw inner leaf).
        if comp.start_byte < cursor || comp.end_byte > len || comp.start_byte >= comp.end_byte {
            continue;
        }
        if comp.start_byte > cursor {
            prev = Some(emit_gap(
                &mut tree,
                root,
                prev,
                &doc[cursor..comp.start_byte],
                cursor == 0,
            ));
        }
        prev = Some(emit_component(
            &mut tree,
            root,
            prev,
            comp,
            &doc[comp.start_byte..comp.end_byte],
        ));
        cursor = comp.end_byte;
    }
    if cursor < len {
        prev = Some(emit_gap(
            &mut tree,
            root,
            prev,
            &doc[cursor..len],
            cursor == 0,
        ));
    }
    let _ = prev;
    tree
}

/// A span the overlay parser did not claim: whitespace becomes a single `Trivia`
/// leaf, a leading `---` YAML header becomes a `frontmatter` element wrapping a
/// `Raw` leaf, everything else a `Raw` leaf. Returns the new top-level child id.
fn emit_gap(
    tree: &mut LosslessTreeCrdt,
    parent: TreeNodeId,
    prev: Option<TreeNodeId>,
    text: &str,
    at_start: bool,
) -> TreeNodeId {
    if !text.is_empty() && text.chars().all(char::is_whitespace) {
        return append(tree, parent, prev, leaf(LeafKind::Trivia, text));
    }
    if at_start && text.trim_start().starts_with("---") {
        let fm = append(tree, parent, prev, element("frontmatter"));
        append(tree, fm, None, leaf(LeafKind::Raw, text));
        return fm;
    }
    append(tree, parent, prev, leaf(LeafKind::Raw, text))
}

/// A `<!-- agent:name -->` … `<!-- /agent:name -->` component: an element whose
/// children tile its span. The open/close markers become `Token` leaves and the
/// body a `Raw` leaf when the markers split cleanly; otherwise the whole span is
/// one `Raw` leaf. Either way the children concatenate back to the span.
fn emit_component(
    tree: &mut LosslessTreeCrdt,
    parent: TreeNodeId,
    prev: Option<TreeNodeId>,
    comp: &Component,
    text: &str,
) -> TreeNodeId {
    let el = append(tree, parent, prev, element(&comp.name));
    match marker_split(text) {
        Some((open, inner, close)) => {
            let mut cprev = append(tree, el, None, leaf(LeafKind::Token, open));
            if !inner.is_empty() {
                cprev = append(tree, el, Some(cprev), leaf(LeafKind::Raw, inner));
            }
            append(tree, el, Some(cprev), leaf(LeafKind::Token, close));
        }
        None => {
            append(tree, el, None, leaf(LeafKind::Raw, text));
        }
    }
    el
}

/// Split a component span into `(open_marker, inner, close_marker)` such that the
/// three pieces concatenate back to `text`. Returns `None` (→ one Raw leaf) when
/// the markers are malformed, so losslessness never depends on marker parsing.
fn marker_split(text: &str) -> Option<(&str, &str, &str)> {
    if !text.starts_with("<!--") {
        return None;
    }
    // End of the open marker: through `-->`, plus a trailing newline if present.
    let open_end0 = text.find("-->")? + 3;
    let open_end = if text[open_end0..].starts_with('\n') {
        open_end0 + 1
    } else {
        open_end0
    };
    // Start of the close marker: the last `<!--` in the span.
    let close_start = text.rfind("<!--")?;
    if close_start < open_end || !text[close_start..].contains("-->") {
        return None;
    }
    Some((
        &text[..open_end],
        &text[open_end..close_start],
        &text[close_start..],
    ))
}

/// The result of a shadow round-trip check: whether the tree renders back to the
/// exact source, and diagnostics (rendered length, first differing byte, live
/// node count) for logging when it does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowAudit {
    pub matches: bool,
    pub source_len: usize,
    pub rendered_len: usize,
    /// First byte offset where render diverges from source, if any.
    pub first_diff_byte: Option<usize>,
    pub live_nodes: usize,
}

/// Build the shadow tree for `doc`, render it, and report whether it round-trips.
/// Pure and side-effect free — callers (preflight/session-check) log the result;
/// it never influences closeout.
pub fn shadow_audit(doc: &str) -> ShadowAudit {
    let tree = parse(doc);
    let rendered = tree.render();
    let first_diff_byte = first_diff(doc.as_bytes(), rendered.as_bytes());
    ShadowAudit {
        matches: first_diff_byte.is_none() && doc.len() == rendered.len(),
        source_len: doc.len(),
        rendered_len: rendered.len(),
        first_diff_byte,
        live_nodes: tree.live_node_count(),
    }
}

impl ShadowAudit {
    /// A stable, single-line `key=value` summary for `ops.log`. On a mismatch it
    /// carries the first differing byte and both lengths so a divergence can be
    /// triaged from the log alone. Phase 1 shadow projection: logged, never acted
    /// on (`#lzlosstree`).
    pub fn ops_log_line(&self, source: &str) -> String {
        if self.matches {
            format!(
                "lossless_shadow source={source} match=true src_len={} nodes={}",
                self.source_len, self.live_nodes,
            )
        } else {
            format!(
                "lossless_shadow source={source} match=false first_diff={} src_len={} rendered_len={} nodes={}",
                self.first_diff_byte
                    .map(|b| b.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                self.source_len,
                self.rendered_len,
                self.live_nodes,
            )
        }
    }
}

/// Build the shadow tree for `doc` and return the `ops.log` line describing whether
/// it round-trips (see [`ShadowAudit::ops_log_line`]). Pure; the caller decides
/// where to log. This is the Phase 1 shadow-projection entry point for preflight /
/// session-check.
pub fn shadow_audit_ops_log_line(doc: &str, source: &str) -> String {
    shadow_audit(doc).ops_log_line(source)
}

/// Replace the inner body — the text between the open and close markers — of the
/// first top-level `<!-- agent:NAME -->` component with `new_inner`, returning the
/// rendered document. Returns `None` when no such component exists, or when the
/// component is opaque/malformed (its markers did not split cleanly, so it is a
/// single `Raw` leaf) — the caller falls back to the legacy path in that case, per
/// the rollout plan's "fall back if the touched region is raw/error" rule.
///
/// Phase 2 primitive (`#lzlosstree`): an agent-owned mutation (status update, queue
/// item edit, …) expressed as a **bounded** lossless-tree edit instead of a
/// whole-document string rewrite. Lossless by construction — only the target
/// component's body leaf changes, so every byte outside that component span is
/// byte-identical in the result. NOT yet wired into the live write authority path;
/// byte-parity with `template::apply_patches` replace mode is the gate before that.
pub fn replace_component_inner(doc: &str, name: &str, new_inner: &str) -> Option<String> {
    let mut tree = parse(doc);
    let el = tree
        .children(TreeNodeId::ROOT)
        .into_iter()
        .find(|&c| tree.element_kind(c) == Some(name))?;
    let kids = tree.children(el);
    // A cleanly-split component's children are [Token(open), Raw(inner)?, Token(close)].
    // If the first child is not the open-marker Token, the markers did not split
    // (opaque/malformed span) and editing would eat marker bytes — bail to legacy.
    let open = *kids.first()?;
    if tree.leaf_kind(open) != Some(LeafKind::Token) {
        return None;
    }
    match kids
        .iter()
        .copied()
        .find(|&k| tree.leaf_kind(k) == Some(LeafKind::Raw))
    {
        Some(body) => {
            let cur = tree.leaf_text(body).ok()?;
            tree.edit_leaf(body, 0, cur.len(), new_inner).ok()?;
        }
        None => {
            // Empty component (open marker directly followed by close): insert a Raw
            // body leaf right after the open marker. Nothing to do for an empty body.
            if new_inner.is_empty() {
                return Some(tree.render());
            }
            tree.create_node(
                el,
                Some(open),
                NodeSeed::Leaf {
                    kind: LeafKind::Raw,
                    text: new_inner.to_string(),
                },
            )
            .ok()?;
        }
    }
    Some(tree.render())
}

fn first_diff(a: &[u8], b: &[u8]) -> Option<usize> {
    let n = a.len().min(b.len());
    for i in 0..n {
        if a[i] != b[i] {
            return Some(i);
        }
    }
    if a.len() != b.len() { Some(n) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Phase 0 round-trip corpus: representative session docs plus corrupt /
    /// intermediate editor buffers. Every one must satisfy render(parse) == source.
    fn corpus() -> Vec<(&'static str, &'static str)> {
        vec![
            ("empty", ""),
            ("whitespace_only", "\n\n   \n"),
            ("prose_only", "just some prose\nwith two lines\n"),
            ("no_trailing_newline", "a line with no newline"),
            (
                "frontmatter_and_component",
                "---\ntitle: plan\n---\n\n<!-- agent:exchange -->\nUser prompt.\n<!-- /agent:exchange -->\n",
            ),
            (
                "multiple_components_with_gaps",
                "intro\n\n<!-- agent:backlog -->\n1. [ ] [#a1] do a thing\n<!-- /agent:backlog -->\n\nmiddle prose\n\n<!-- agent:status -->\nok\n<!-- /agent:status -->\n",
            ),
            (
                "multibyte_content",
                "café ☕ prix: 12€\n\n<!-- agent:exchange -->\nTítulo — héllo 世界\n<!-- /agent:exchange -->\n",
            ),
            (
                "unclosed_marker_corrupt",
                "before\n<!-- agent:exchange -->\ndangling with no close marker\nmore text\n",
            ),
            (
                "empty_inner_component",
                "<!-- agent:log -->\n<!-- /agent:log -->\n",
            ),
            (
                "crlf_and_tabs",
                "line1\r\n\tindented\r\n<!-- agent:queue -->\r\ndo [#x]\r\n<!-- /agent:queue -->\r\n",
            ),
            (
                "adjacent_components_no_gap",
                "<!-- agent:status -->\nS\n<!-- /agent:status --><!-- agent:log -->\nL\n<!-- /agent:log -->\n",
            ),
        ]
    }

    #[test]
    fn round_trips_the_whole_corpus() {
        for (name, doc) in corpus() {
            let audit = shadow_audit(doc);
            assert!(
                audit.matches,
                "{name}: render(parse(doc)) != doc; first diff at {:?} (src_len={}, rendered_len={})",
                audit.first_diff_byte, audit.source_len, audit.rendered_len
            );
            // Redundant direct check against the live tree render.
            assert_eq!(parse(doc).render(), doc, "{name}: direct render mismatch");
        }
    }

    #[test]
    fn well_formed_component_yields_token_and_element_nodes() {
        let doc = "<!-- agent:exchange -->\nbody\n<!-- /agent:exchange -->\n";
        let tree = parse(doc);
        let top = tree.children(TreeNodeId::ROOT);
        // First top-level child is the exchange element.
        assert_eq!(tree.element_kind(top[0]), Some("exchange"));
        let kids = tree.children(top[0]);
        assert_eq!(tree.leaf_kind(kids[0]), Some(LeafKind::Token)); // open marker
        assert_eq!(tree.leaf_kind(kids[1]), Some(LeafKind::Raw)); // body
        assert_eq!(tree.leaf_kind(*kids.last().unwrap()), Some(LeafKind::Token)); // close marker
        assert_eq!(tree.render(), doc);
    }

    #[test]
    fn frontmatter_becomes_an_element() {
        let doc = "---\ntitle: t\n---\nbody\n";
        let tree = parse(doc);
        let top = tree.children(TreeNodeId::ROOT);
        assert_eq!(tree.element_kind(top[0]), Some("frontmatter"));
        assert_eq!(tree.render(), doc);
    }

    #[test]
    fn ops_log_line_reports_match_and_mismatch() {
        // A round-tripping document reports match=true with no diff fields.
        let line =
            shadow_audit_ops_log_line("<!-- agent:log -->\nx\n<!-- /agent:log -->\n", "initial");
        assert!(
            line.starts_with("lossless_shadow source=initial match=true"),
            "{line}"
        );
        assert!(line.contains("src_len="), "{line}");
        assert!(line.contains("nodes="), "{line}");
        assert!(
            !line.contains("first_diff="),
            "match line must omit diff fields: {line}"
        );

        // A hand-built mismatch renders match=false with a first_diff byte.
        let audit = ShadowAudit {
            matches: false,
            source_len: 10,
            rendered_len: 8,
            first_diff_byte: Some(5),
            live_nodes: 3,
        };
        let line = audit.ops_log_line("session_check");
        assert!(line.contains("source=session_check match=false"), "{line}");
        assert!(line.contains("first_diff=5"), "{line}");
        assert!(line.contains("rendered_len=8"), "{line}");
    }

    #[test]
    fn replace_component_inner_changes_only_the_target_span() {
        let doc = "intro\n\n<!-- agent:status -->\nold status\n<!-- /agent:status -->\n\n<!-- agent:log -->\nkeep me\n<!-- /agent:log -->\n";
        let out = replace_component_inner(doc, "status", "new status\n").expect("status exists");
        // The result is a valid, self-consistent lossless document.
        assert_eq!(parse(&out).render(), out);
        // The new body is present; the old body is gone.
        assert!(out.contains("new status"), "{out}");
        assert!(!out.contains("old status"), "{out}");
        // Every byte outside the status component is untouched: prose, the log
        // component body, and both marker lines survive verbatim.
        assert!(out.starts_with("intro\n\n<!-- agent:status -->\n"), "{out}");
        assert!(
            out.contains(
                "<!-- /agent:status -->\n\n<!-- agent:log -->\nkeep me\n<!-- /agent:log -->\n"
            ),
            "{out}"
        );
    }

    #[test]
    fn replace_component_inner_fills_an_empty_component() {
        let doc = "<!-- agent:log -->\n<!-- /agent:log -->\n";
        let out = replace_component_inner(doc, "log", "first line\n").expect("log exists");
        assert_eq!(out, "<!-- agent:log -->\nfirst line\n<!-- /agent:log -->\n");
        assert_eq!(parse(&out).render(), out);
    }

    #[test]
    fn replace_component_inner_declines_missing_or_malformed() {
        // No such component -> None (caller keeps legacy path).
        assert_eq!(
            replace_component_inner(
                "<!-- agent:status -->\nx\n<!-- /agent:status -->\n",
                "log",
                "y"
            ),
            None
        );
        // Unclosed/opaque component -> single Raw leaf, no clean markers -> None,
        // so the primitive never risks eating marker bytes.
        assert_eq!(
            replace_component_inner(
                "<!-- agent:status -->\ndangling no close\nmore\n",
                "status",
                "y"
            ),
            None
        );
    }

    #[test]
    fn corrupt_input_stays_bounded_and_lossless() {
        // A pathological buffer: half-open markers, stray delimiters, multibyte.
        let doc = "<!-- agent:\n</agent> ``` café <!-- /agent:x --> \u{fffd}\n";
        let audit = shadow_audit(doc);
        assert!(audit.matches, "corrupt buffer must still round-trip");
    }
}
