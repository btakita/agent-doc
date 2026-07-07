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
use lazily::{LeafKind, LosslessTreeCrdt, NodeSeed, TreeNodeId, TreeUpdate, TreeVersionFrontier};
use serde::{Deserialize, Serialize};

/// The peer id every agent-doc projection replica is built under. A durable
/// projection is a single-replica record, so the peer is fixed; cross-replica
/// identity is negotiated later at the editor/relay layer.
const PROJECTION_PEER: u64 = 1;

/// A durable, serde-serializable projection of a session document as a lossless
/// tree (`#lzlosstree` Phase 3). It carries the full op-stream (so the tree — and
/// therefore the exact document text — can be rebuilt) plus the SHA-256 of the
/// rendered text, which is the cheap staleness proof: a projection may only be
/// trusted to reconstruct current text when its `rendered_sha256` still matches the
/// editor-visible document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LosslessProjection {
    /// Every op the projected tree holds — a full snapshot, replayable onto a fresh
    /// replica. `diff` against the empty frontier yields all ops.
    pub ops: TreeUpdate,
    /// SHA-256 of `render(tree)` == the source document, for staleness proof.
    pub rendered_sha256: String,
    /// Byte length of the rendered document (cheap pre-check before hashing).
    pub rendered_len: usize,
}

impl LosslessProjection {
    /// Whether this projection still describes `visible_text` — the frontier/hash
    /// proof the rollout plan requires before a projection may reconstruct current
    /// document text. A stale projection (hash mismatch) must never override the
    /// editor-visible document.
    pub fn is_current_for(&self, visible_text: &str) -> bool {
        self.rendered_len == visible_text.len()
            && self.rendered_sha256 == agent_doc_hash::content_hash(visible_text)
    }
}

/// Build a durable projection of `doc`: parse it losslessly, snapshot the full
/// op-stream, and record the rendered-text hash. `restore(project(doc)) == doc` by
/// construction, since the tree renders back to the source.
pub fn project(doc: &str) -> LosslessProjection {
    let tree = parse(doc);
    LosslessProjection {
        ops: tree.diff(&TreeVersionFrontier::default()),
        rendered_sha256: agent_doc_hash::content_hash(doc),
        rendered_len: doc.len(),
    }
}

/// Rebuild the document text from a durable projection by replaying its op-stream
/// onto a fresh replica and rendering. This is the Phase 3 recovery path: current
/// text can be reconstructed from the projection alone.
pub fn restore(projection: &LosslessProjection) -> String {
    let mut tree = LosslessTreeCrdt::new(PROJECTION_PEER);
    tree.apply_update(&projection.ops);
    tree.render()
}

/// Serialize a projection to durable JSON bytes for on-disk storage.
pub fn projection_to_bytes(projection: &LosslessProjection) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(projection)
}

/// Parse a projection from durable JSON bytes.
pub fn projection_from_bytes(bytes: &[u8]) -> Result<LosslessProjection, serde_json::Error> {
    serde_json::from_slice(bytes)
}

/// The directory (relative to a project root) where the binary drops lossless-tree
/// **frames** for a tree-capable editor to apply (`#lzlosstree` Phase 5). This is the
/// dedicated tree-update transport the rollout plan calls for; it is separate from
/// the flat `.agent-doc/patches/` channel so a non-capable plugin never sees frames.
pub const LOSSLESS_FRAME_DIR: &str = ".agent-doc/lossless-frames";

/// Write a lossless-tree frame for `content` into `frame_path` (a
/// `<root>/.agent-doc/lossless-frames/<hash>.json` file): project `content` and
/// serialize the projection. The capable editor renders it with [`read_frame_render`]
/// and applies the result to its buffer. Creates the parent directory as needed.
pub fn write_frame(frame_path: &std::path::Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = frame_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let projection = project(content);
    let bytes = projection_to_bytes(&projection)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(frame_path, bytes)
}

/// Read a lossless-tree frame file and render it back to document text — the editor's
/// apply side of the Phase 5 transport. Returns `Ok(None)` when the file is absent;
/// an unparseable frame is an `InvalidData` error (the editor keeps its buffer).
pub fn read_frame_render(frame_path: &std::path::Path) -> std::io::Result<Option<String>> {
    let bytes = match std::fs::read(frame_path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let projection = projection_from_bytes(&bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(restore(&projection)))
}

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

/// The full rendered text of `node`'s subtree (a leaf's own text, or the in-order
/// concatenation of its descendants' text). Mirrors [`LosslessTreeCrdt::render`]
/// scoped to one node.
fn render_node(tree: &LosslessTreeCrdt, node: TreeNodeId) -> String {
    if let Ok(text) = tree.leaf_text(node) {
        return text;
    }
    tree.children(node)
        .into_iter()
        .map(|c| render_node(tree, c))
        .collect()
}

/// One top-level structural slot of a document: either literal framing (trivia,
/// prose, frontmatter — text that must match across merge sides) or a component
/// whose body may be independently merged. Two documents are "frame-compatible"
/// when their `Struct` sequences are equal — only component bodies then differ.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Struct {
    Lit(String),
    Comp {
        name: String,
        open: String,
        close: String,
    },
}

/// Split a document into its top-level structure and the ordered component body
/// texts. Returns `None` if any component is opaque/malformed (no clean marker
/// tokens), so a merge over it degrades to the legacy path rather than guessing.
fn decompose(doc: &str) -> Option<(Vec<Struct>, Vec<String>)> {
    let tree = parse(doc);
    let mut structure = Vec::new();
    let mut bodies = Vec::new();
    for child in tree.children(TreeNodeId::ROOT) {
        match tree.element_kind(child) {
            // `frontmatter` is framing, not a mergeable agent component: treat its
            // whole subtree as an opaque literal so a frontmatter change fails the
            // frame-compatibility check and falls back to the legacy merge.
            Some(kind) if kind != "frontmatter" => {
                let kids = tree.children(child);
                let open = *kids.first()?;
                let close = *kids.last()?;
                if tree.leaf_kind(open) != Some(LeafKind::Token)
                    || tree.leaf_kind(close) != Some(LeafKind::Token)
                {
                    return None;
                }
                let body = kids
                    .iter()
                    .copied()
                    .find(|&k| tree.leaf_kind(k) == Some(LeafKind::Raw))
                    .map(|k| tree.leaf_text(k).unwrap_or_default())
                    .unwrap_or_default();
                structure.push(Struct::Comp {
                    name: kind.to_string(),
                    open: tree.leaf_text(open).ok()?,
                    close: tree.leaf_text(close).ok()?,
                });
                bodies.push(body);
            }
            _ => structure.push(Struct::Lit(render_node(&tree, child))),
        }
    }
    Some((structure, bodies))
}

/// Conservative per-component 3-way merge over the lossless tree
/// (`#lzlosstree` / `#qcellmerge1`). Returns the merged document when the three
/// sides share identical framing (same literal slots and component markers, in
/// order) and every component body merges unambiguously; otherwise `None`, so the
/// caller keeps the authoritative legacy `merge_by_component` result.
///
/// Per component the rule is the standard 3-way: take the side that changed the
/// body, or either when both made the *same* change, and bail (`None`) on a true
/// both-sides-diverged conflict. The result is reconstructed from the shared
/// framing, so it is lossless by construction. NOT an authority path yet — wired
/// as a parity shadow beside the existing merge until proven on live merges.
pub fn merge_via_lossless_tree(base: &str, ours: &str, theirs: &str) -> Option<String> {
    if ours == theirs {
        return Some(ours.to_string());
    }
    let (base_struct, base_bodies) = decompose(base)?;
    let (ours_struct, ours_bodies) = decompose(ours)?;
    let (theirs_struct, theirs_bodies) = decompose(theirs)?;
    // Framing (literals + markers + component order) must be identical on all three
    // sides; only component bodies may differ. Anything else → legacy merge.
    if base_struct != ours_struct || base_struct != theirs_struct {
        return None;
    }
    debug_assert_eq!(base_bodies.len(), ours_bodies.len());
    debug_assert_eq!(base_bodies.len(), theirs_bodies.len());
    let mut merged_bodies = Vec::with_capacity(base_bodies.len());
    for ((b, o), t) in base_bodies.iter().zip(&ours_bodies).zip(&theirs_bodies) {
        let merged = if o == b {
            t
        } else if t == b || o == t {
            // theirs unchanged, or both sides made the identical edit.
            o
        } else {
            return None; // genuine conflict — let the legacy CRDT resolve it
        };
        merged_bodies.push(merged.clone());
    }
    // Reconstruct from the shared framing with the merged bodies.
    let mut out = String::new();
    let mut ci = 0;
    for item in &base_struct {
        match item {
            Struct::Lit(text) => out.push_str(text),
            Struct::Comp { open, close, .. } => {
                out.push_str(open);
                out.push_str(&merged_bodies[ci]);
                out.push_str(close);
                ci += 1;
            }
        }
    }
    Some(out)
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

    /// The load-bearing equivalence for Phase 2: for a clean-marker component the
    /// tree's bounded body replace is byte-identical to the real element parser's
    /// `replace_content`. This is what lets `replace_component_inner` stand in for
    /// `element::replace_content` at every component-body write site that currently
    /// feeds the full-document CRDT — a wholesale drop-in, not a per-path shadow.
    #[test]
    fn replace_component_inner_matches_element_replace_content() {
        let docs = [
            "<!-- agent:status -->\nold\n<!-- /agent:status -->\n",
            "---\ntitle: t\n---\n\n<!-- agent:status patch=replace -->\nold status\n<!-- /agent:status -->\n\n<!-- agent:log -->\nkeep\n<!-- /agent:log -->\n",
            "intro prose\n\n<!-- agent:queue -->\ndo [#x]\n<!-- /agent:queue -->\ntail\n",
            "<!-- agent:log -->\n<!-- /agent:log -->\n", // empty inner
            "<!-- agent:status -->\ncafé ☕ 世界\n<!-- /agent:status -->\n", // multibyte
        ];
        let replacements = [
            "\nNEW\n",
            "\nmulti\nline\n",
            "",
            "\n",
            "just text no newlines",
        ];
        for doc in docs {
            let comps = agent_doc_element::element::parse(doc).expect("element parse");
            for comp in &comps {
                for new_content in replacements {
                    let legacy = comp.replace_content(doc, new_content);
                    let tree = replace_component_inner(doc, &comp.name, new_content)
                        .unwrap_or_else(|| panic!("tree declined {}={new_content:?}", comp.name));
                    assert_eq!(
                        tree, legacy,
                        "tree/element divergence for component {} content {new_content:?}",
                        comp.name
                    );
                    // And the tree result is itself lossless.
                    assert_eq!(parse(&tree).render(), tree);
                }
            }
        }
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

    // A base doc with two independent components either side can edit.
    const MERGE_BASE: &str = "---\ntitle: t\n---\n\n<!-- agent:status -->\nbase status\n<!-- /agent:status -->\n\nprose\n\n<!-- agent:log -->\nbase log\n<!-- /agent:log -->\n";

    #[test]
    fn merge_via_lossless_tree_merges_disjoint_component_edits() {
        // ours edits status, theirs edits log → both changes land, framing intact.
        let ours = MERGE_BASE.replace("base status", "our status");
        let theirs = MERGE_BASE.replace("base log", "their log");
        let merged = merge_via_lossless_tree(MERGE_BASE, &ours, &theirs).expect("clean merge");
        assert!(merged.contains("our status"), "{merged}");
        assert!(merged.contains("their log"), "{merged}");
        assert!(!merged.contains("base status"), "{merged}");
        assert!(!merged.contains("base log"), "{merged}");
        assert_eq!(parse(&merged).render(), merged); // lossless
    }

    #[test]
    fn merge_via_lossless_tree_one_sided_and_identical() {
        let ours = MERGE_BASE.replace("base status", "our status");
        // theirs == base → result is ours.
        assert_eq!(
            merge_via_lossless_tree(MERGE_BASE, &ours, MERGE_BASE).as_deref(),
            Some(ours.as_str())
        );
        // ours == base → result is theirs.
        assert_eq!(
            merge_via_lossless_tree(MERGE_BASE, MERGE_BASE, &ours).as_deref(),
            Some(ours.as_str())
        );
        // both made the SAME edit → that edit (no conflict).
        assert_eq!(
            merge_via_lossless_tree(MERGE_BASE, &ours, &ours).as_deref(),
            Some(ours.as_str())
        );
    }

    #[test]
    fn merge_via_lossless_tree_declines_conflict_and_reframe() {
        // Both sides edit the SAME component differently → genuine conflict → None.
        let ours = MERGE_BASE.replace("base status", "our status");
        let theirs = MERGE_BASE.replace("base status", "their status");
        assert_eq!(merge_via_lossless_tree(MERGE_BASE, &ours, &theirs), None);
        // A framing change (a component added) → None (legacy handles structure).
        let reframed =
            format!("{MERGE_BASE}\n<!-- agent:queue -->\ndo [#x]\n<!-- /agent:queue -->\n");
        assert_eq!(
            merge_via_lossless_tree(MERGE_BASE, &reframed, MERGE_BASE),
            None
        );
        // Frontmatter change → None (frontmatter is framing).
        let fm_changed = MERGE_BASE.replace("title: t", "title: changed");
        assert_eq!(
            merge_via_lossless_tree(MERGE_BASE, &fm_changed, MERGE_BASE),
            None
        );
    }

    /// Byte-parity cross-check against the real yrs-backed `merge_by_component`:
    /// where the lossless-tree merge returns `Some`, it must equal the authoritative
    /// result for these clean, non-conflicting cases. This is the evidence the
    /// merge-layer shadow needs before any authority flip (#lzlosstree).
    #[test]
    fn merge_via_lossless_tree_matches_merge_by_component_on_clean_cases() {
        use agent_doc_merge::crdt::{CrdtDoc, merge_by_component};
        let base_state = CrdtDoc::from_text(MERGE_BASE).encode_state();
        let cases = [
            // (ours, theirs)
            (
                MERGE_BASE.replace("base status", "our status"),
                MERGE_BASE.replace("base log", "their log"),
            ),
            (
                MERGE_BASE.replace("base status", "our status"),
                MERGE_BASE.to_string(),
            ),
            (
                MERGE_BASE.to_string(),
                MERGE_BASE.replace("base log", "their log"),
            ),
        ];
        for (ours, theirs) in cases {
            let legacy = merge_by_component(Some(&base_state), &ours, &theirs).unwrap();
            let tree = merge_via_lossless_tree(MERGE_BASE, &ours, &theirs)
                .expect("clean case should merge");
            assert_eq!(
                tree, legacy,
                "lossless-tree merge diverged from merge_by_component\nours={ours:?}\ntheirs={theirs:?}"
            );
        }
    }

    #[test]
    fn projection_round_trips_the_corpus_and_survives_serialization() {
        for (name, doc) in corpus() {
            let projection = project(doc);
            // Recovery rebuilds the exact document text from the projection alone.
            assert_eq!(restore(&projection), doc, "{name}: restore != source");
            // The projection proves it describes the current document.
            assert!(projection.is_current_for(doc), "{name}: should be current");
            // Durable bytes round-trip losslessly, and still reconstruct the doc.
            let bytes = projection_to_bytes(&projection).expect("serialize");
            let restored = projection_from_bytes(&bytes).expect("deserialize");
            assert_eq!(
                restored.rendered_sha256, projection.rendered_sha256,
                "{name}: hash"
            );
            assert_eq!(
                restored.rendered_len, projection.rendered_len,
                "{name}: len"
            );
            assert_eq!(restore(&restored), doc, "{name}: restore after serde");
        }
    }

    #[test]
    fn frame_write_then_render_round_trips_through_a_file() {
        let dir = std::env::temp_dir().join(format!("lzframe-{}", std::process::id()));
        let frame = dir.join(LOSSLESS_FRAME_DIR).join("abc.json");
        let content = "<!-- agent:status -->\nframe body\n<!-- /agent:status -->\n";
        // Server side: emit the frame.
        write_frame(&frame, content).expect("write frame");
        // Editor side: render it back to the exact document text.
        let rendered = read_frame_render(&frame)
            .expect("read frame")
            .expect("frame present");
        assert_eq!(rendered, content);
        // Absent frame → Ok(None).
        let missing = dir.join(LOSSLESS_FRAME_DIR).join("missing.json");
        assert_eq!(read_frame_render(&missing).unwrap(), None);
        // A corrupt frame is a hard error (editor keeps its buffer), never a silent wrong render.
        std::fs::write(&frame, b"not a projection").unwrap();
        assert!(read_frame_render(&frame).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn projection_staleness_guard_rejects_a_moved_on_document() {
        let doc = "<!-- agent:status -->\noriginal\n<!-- /agent:status -->\n";
        let projection = project(doc);
        assert!(projection.is_current_for(doc));
        // The editor-visible document moved forward: the projection is now stale and
        // must not be trusted to override the visible text.
        let moved_on = "<!-- agent:status -->\nedited by operator\n<!-- /agent:status -->\n";
        assert!(
            !projection.is_current_for(moved_on),
            "stale projection must not claim to describe moved-on text"
        );
        // A same-length but different edit is also caught by the hash (not just len).
        let same_len_diff = "<!-- agent:status -->\noriginaL\n<!-- /agent:status -->\n";
        assert_eq!(same_len_diff.len(), doc.len());
        assert!(
            !projection.is_current_for(same_len_diff),
            "hash must catch same-len edits"
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
