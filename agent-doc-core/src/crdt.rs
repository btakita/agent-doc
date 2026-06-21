//! # Module: crdt
//!
//! ## Spec
//! - `CrdtDoc::from_text(content)`: construct a new Yrs document pre-populated with text.
//! - `CrdtDoc::to_text()`: extract current UTF-8 text from the document.
//! - `CrdtDoc::apply_edit(offset, delete_len, insert)`: apply a local delta edit (delete then
//!   insert at byte offset) within a single transaction.
//! - `CrdtDoc::encode_state()`: serialize full document state as a v1 update byte vector for
//!   persistence or cross-doc merge.
//! - `CrdtDoc::decode_state(bytes)`: deserialize a previously encoded state into a new doc.
//! - `merge(base_state, ours_text, theirs_text)`: three-way CRDT merge.
//!   1. Decodes base state (or starts from empty if `None`).
//!   2. Detects stale base: if base shares <50% common prefix with both sides, replaces base
//!      with `ours_text` to prevent duplicate insertions.
//!   3. Advances base to the line-snapped common prefix of ours and theirs when that prefix
//!      extends beyond the current base — prevents duplication of shared new content.
//!   4. Computes `similar`-based line diffs from base → ours and base → theirs.
//!   5. Replays diffs onto two independent Yrs docs (agent=client_id 1, user=client_id 2).
//!   6. Merges by applying theirs' incremental update into the agent doc.
//!   7. Runs `dedup_adjacent_blocks` to remove identical adjacent paragraph-level blocks.
//!   8. Returns the merged string (always conflict-free).
//! - `dedup_adjacent_blocks(text)`: removes duplicate adjacent blocks (separated by `\n\n`)
//!   where a block has ≥1 non-empty line, to clean up CRDT double-insertion artifacts.
//!   Structural blocks (thematic breaks like `---`) are exempt from dedup.
//! - `compact(state)`: decode then re-encode a CRDT state to GC tombstones.
//!
//! ## Agentic Contracts
//! - `merge` is always conflict-free — it never produces conflict markers.
//! - Agent content (client_id 1) appears before user content (client_id 2) at identical
//!   insertion points due to Yrs' deterministic client-ID ordering.
//! - Short-circuit: if `ours_text == theirs_text`, `merge` returns immediately without any
//!   CRDT operations.
//! - Stale base detection prevents duplicate insertions across multiple merge cycles.
//! - Shared common prefix (line-boundary snapped) is never inserted twice.
//! - `dedup_adjacent_blocks` removes blocks with ≥1 non-empty line; structural blocks
//!   (thematic breaks like `---`, `***`, `___`) are exempt to avoid false positives.
//! - `encode_state` / `decode_state` are inverse operations: round-trip preserves all text.
//! - `compact` is idempotent: compacting already-compact state is a no-op in terms of text.
//!
//! ## Evals
//! - `roundtrip_text`: `from_text(s).to_text() == s` for arbitrary content.
//! - `encode_decode_roundtrip`: encode then decode preserves text exactly.
//! - `apply_edit_insert`: insert at offset 0 prepends correctly.
//! - `apply_edit_delete`: delete range removes exact byte count.
//! - `apply_edit_replace`: delete + insert at same offset replaces content.
//! - `merge_both_append`: both sides add different text → both present, no conflict.
//! - `merge_agent_ordering`: agent and user insert at same position → agent content first.
//! - `merge_identical_sides`: ours == theirs → short-circuit, result equals either side.
//! - `merge_no_base_state`: `None` base → valid merged result, both sides present.
//! - `merge_stale_base_no_duplicates`: base is stale (< 50% common prefix) → no duplication.
//! - `merge_shared_prefix_no_duplication`: ours and theirs share new content beyond base →
//!   shared content appears exactly once.
//! - `dedup_adjacent_blocks_removes_duplicate`: two identical adjacent multi-line blocks →
//!   deduplicated to one.
//! - `dedup_adjacent_blocks_preserves_structural`: structural (thematic break) repeated blocks left intact.
//! - `dedup_adjacent_blocks_removes_single_line_content`: single-line content duplicates are removed.
//! - `compact_preserves_text`: compact state → decoded text unchanged.

use anyhow::{Context, Result};
use yrs::updates::decoder::Decode;
use yrs::{Doc, GetString, ReadTxn, Text, TextRef, Transact, Update};

const TEXT_KEY: &str = "content";

/// CRDT document wrapping a Yjs `Doc` for conflict-free merging.
pub struct CrdtDoc {
    doc: Doc,
}

impl CrdtDoc {
    /// Create a new CRDT document initialized with the given text content.
    pub fn from_text(content: &str) -> Self {
        let doc = Doc::new();
        let text = doc.get_or_insert_text(TEXT_KEY);
        let mut txn = doc.transact_mut();
        text.insert(&mut txn, 0, content);
        drop(txn);
        CrdtDoc { doc }
    }

    /// Extract the current text content from the CRDT document.
    pub fn to_text(&self) -> String {
        let text = self.doc.get_or_insert_text(TEXT_KEY);
        let txn = self.doc.transact();
        text.get_string(&txn)
    }

    /// Apply a local edit: delete `delete_len` chars at `offset`, then insert `insert` there.
    #[allow(dead_code)] // Used in tests and Phase 4 stream write-back
    pub fn apply_edit(&self, offset: u32, delete_len: u32, insert: &str) {
        let text = self.doc.get_or_insert_text(TEXT_KEY);
        let mut txn = self.doc.transact_mut();
        if delete_len > 0 {
            text.remove_range(&mut txn, offset, delete_len);
        }
        if !insert.is_empty() {
            text.insert(&mut txn, offset, insert);
        }
    }

    /// Encode the full document state (for persistence).
    pub fn encode_state(&self) -> Vec<u8> {
        let txn = self.doc.transact();
        txn.encode_state_as_update_v1(&yrs::StateVector::default())
    }

    /// Decode a previously encoded state into a new CrdtDoc.
    pub fn decode_state(bytes: &[u8]) -> Result<Self> {
        let doc = Doc::new();
        let update = Update::decode_v1(bytes)
            .map_err(|e| anyhow::anyhow!("failed to decode CRDT state: {}", e))?;
        let mut txn = doc.transact_mut();
        txn.apply_update(update)
            .map_err(|e| anyhow::anyhow!("failed to apply CRDT update: {}", e))?;
        drop(txn);
        Ok(CrdtDoc { doc })
    }
}

/// Merge two concurrent text versions against a common base using CRDT.
///
/// Creates three CRDT actors: base, ours, theirs.
/// Applies each side's edits as diffs from the base, then merges updates.
/// Returns the merged text (conflict-free).
///
/// **Stale base detection:** If the CRDT base text doesn't match either ours
/// or theirs as a prefix/substring, the base is stale. In that case, we use
/// `ours_text` as the base to prevent duplicate insertions.
pub fn merge(base_state: Option<&[u8]>, ours_text: &str, theirs_text: &str) -> Result<String> {
    // Short-circuit: if both sides are identical, no merge needed
    if ours_text == theirs_text {
        eprintln!("[crdt] ours == theirs, skipping merge");
        return Ok(ours_text.to_string());
    }

    // Bootstrap base doc from state or empty
    let base_doc = if let Some(bytes) = base_state {
        CrdtDoc::decode_state(bytes).context("failed to decode base CRDT state")?
    } else {
        CrdtDoc::from_text("")
    };
    let mut base_text = base_doc.to_text();

    eprintln!(
        "[crdt] merge: base_len={} ours_len={} theirs_len={}",
        base_text.len(),
        ours_text.len(),
        theirs_text.len()
    );

    // Capture committed assistant response headings from the *original* base,
    // before stale-base detection / prefix advancement can rewrite `base_text`.
    // Committed `### Re:` blocks are append-only history; a stale or divergent
    // `theirs` must not be able to delete them out of the merged result.
    // (#ipc-crdt-response-drift)
    let committed_response_headings = response_headings(&base_text);

    // Stale base detection: if the base text doesn't share enough content
    // with both sides, it's stale. Use ours as the base instead.
    // This prevents duplicate insertions when both sides contain text
    // that the stale base doesn't have.
    //
    // We check both common prefix AND common suffix. Template documents
    // have structural content (frontmatter, component markers, pending
    // sections) that bookend the exchange. When the exchange is short or
    // empty, only checking the prefix classifies a correct base as stale
    // (the suffix — closing markers + pending — goes uncounted).
    let ours_prefix = common_prefix_len(&base_text, ours_text);
    let ours_suffix = common_suffix_len(&base_text, ours_text);
    let theirs_prefix = common_prefix_len(&base_text, theirs_text);
    let theirs_suffix = common_suffix_len(&base_text, theirs_text);
    let base_len = base_text.len();

    // Clamp shared bytes to base_len (prefix + suffix can overlap for short strings)
    let ours_shared = (ours_prefix + ours_suffix).min(base_len);
    let theirs_shared = (theirs_prefix + theirs_suffix).min(base_len);

    if base_len > 0
        && (ours_shared as f64 / base_len as f64) < 0.5
        && (theirs_shared as f64 / base_len as f64) < 0.5
    {
        eprintln!(
            "[crdt] Stale CRDT base detected (shared: ours={}%, theirs={}%). Using ours as base.",
            (ours_shared * 100) / base_len,
            (theirs_shared * 100) / base_len
        );
        base_text = ours_text.to_string();
    }

    // Advance base to the common prefix of ours and theirs when it extends
    // beyond the point where the base diverges from either side.
    //
    // When both ours and theirs independently added the same text beyond the
    // base (e.g., both contain a user prompt that the base doesn't have),
    // the CRDT treats each insertion as independent and includes both, causing
    // duplication. Fix: use the common prefix of ours and theirs as the effective
    // base, so shared additions are not treated as independent insertions.
    //
    // The divergence point (`base_diverge`) is where the base stops matching
    // either side. Content shared by ours and theirs beyond this point was
    // added by both sides independently and must be promoted into the base.
    //
    // This handles two patterns:
    //   Pattern 1 (mutual prefix > base length):
    //     base   = "old content"
    //     ours   = "old content + user prompt + agent response"
    //     theirs = "old content + user prompt + small edit"
    //   Pattern 2 (base has trailing structure beyond divergence):
    //     base   = "header + empty exchange + footer"
    //     ours   = "header + prompt + response + footer"
    //     theirs = "header + prompt + boundary + footer"
    //   Both diverge from base at "header", but share "prompt" beyond it.
    let mutual_prefix = common_prefix_len(ours_text, theirs_text);
    let base_diverge =
        common_prefix_len(&base_text, ours_text).max(common_prefix_len(&base_text, theirs_text));
    if mutual_prefix > base_diverge {
        // Snap to a line boundary to avoid splitting mid-line/mid-word.
        // Without this, the shared prefix can include partial formatting
        // sequences (e.g., a leading `*` from `**bold**`), causing the
        // CRDT merge to separate that character from the rest of the
        // formatting, producing garbled text like `*Soft-bristle brush only**`
        // instead of `**Soft-bristle brush only**`.
        let snap = &ours_text[..mutual_prefix];
        let snapped = match snap.rfind('\n') {
            Some(pos) if pos >= base_diverge => pos + 1,
            _ => base_diverge, // no suitable line boundary — don't advance
        };
        if snapped > base_diverge {
            eprintln!(
                "[crdt] Advancing base to shared prefix (diverge={} → {})",
                base_diverge, snapped
            );
            base_text = ours_text[..snapped].to_string();
        }
    }

    // Compute diffs from base to each side
    let ours_ops = compute_edit_ops(&base_text, ours_text);
    let theirs_ops = compute_edit_ops(&base_text, theirs_text);

    // Create two independent docs from the base state.
    // If base was overridden (stale detection), rebuild from the new base_text.
    let base_encoded = if base_text == base_doc.to_text() {
        base_doc.encode_state()
    } else {
        CrdtDoc::from_text(&base_text).encode_state()
    };

    // Agent gets lower client ID (1) so Yrs natively places agent content
    // BEFORE human content when both insert at the same position.
    // Yrs orders concurrent inserts by client ID: lower client ID goes first.
    let ours_doc = Doc::with_client_id(1);
    {
        let update =
            Update::decode_v1(&base_encoded).map_err(|e| anyhow::anyhow!("decode error: {}", e))?;
        let mut txn = ours_doc.transact_mut();
        txn.apply_update(update)
            .map_err(|e| anyhow::anyhow!("apply error: {}", e))?;
    }

    let theirs_doc = Doc::with_client_id(2);
    {
        let update =
            Update::decode_v1(&base_encoded).map_err(|e| anyhow::anyhow!("decode error: {}", e))?;
        let mut txn = theirs_doc.transact_mut();
        txn.apply_update(update)
            .map_err(|e| anyhow::anyhow!("apply error: {}", e))?;
    }

    // Apply ours edits
    {
        let text = ours_doc.get_or_insert_text(TEXT_KEY);
        let mut txn = ours_doc.transact_mut();
        apply_ops(&text, &mut txn, &ours_ops);
    }

    // Apply theirs edits
    {
        let text = theirs_doc.get_or_insert_text(TEXT_KEY);
        let mut txn = theirs_doc.transact_mut();
        apply_ops(&text, &mut txn, &theirs_ops);
    }

    // Merge: apply theirs' changes into ours
    let ours_sv = {
        let txn = ours_doc.transact();
        txn.state_vector()
    };
    let theirs_update = {
        let txn = theirs_doc.transact();
        txn.encode_state_as_update_v1(&ours_sv)
    };
    {
        let update = Update::decode_v1(&theirs_update)
            .map_err(|e| anyhow::anyhow!("decode error: {}", e))?;
        let mut txn = ours_doc.transact_mut();
        txn.apply_update(update)
            .map_err(|e| anyhow::anyhow!("apply error: {}", e))?;
    }

    // Read merged result. With agent/ours=client_id(1) and theirs=client_id(2)
    // (see the client-id assignment above), Yrs natively orders concurrent
    // inserts at the same position by ascending client id, so agent content
    // lands before foreign content at the append boundary. No post-merge
    // reorder needed.
    let merged = {
        let text = ours_doc.get_or_insert_text(TEXT_KEY);
        let txn = ours_doc.transact();
        text.get_string(&txn)
    };

    // Committed-response protection (#ipc-crdt-response-drift): a stale or
    // divergent `theirs` (e.g. a concurrent foreign supervisor, or a stale
    // editor live-buffer pushed by convergence) can present a document that has
    // *lost* a `### Re:` response block which was already committed (present in
    // `base`) and is still present in `ours`. The line-diff merge would honor
    // that foreign deletion and strip the committed response from the result
    // ("### Re: header missing" / prior response hoisted away). Foreign writers
    // must not delete committed exchange history.
    let merged =
        guard_committed_responses(merged, &committed_response_headings, ours_text, theirs_text);

    // Post-merge dedup: remove identical adjacent blocks (#15)
    Ok(dedup_adjacent_blocks(&merged))
}

/// One node of the per-component document segmentation (`#qcellmerge1`).
///
/// A document is decomposed into an alternating sequence of interstitial text
/// (preamble, gaps between components, trailing residue) and `<!-- agent:* -->`
/// component regions. Each node merges against its own base independently so
/// unrelated nodes (e.g. `exchange` vs `queue`) never contend and content can
/// never splice across components.
#[derive(Debug, Clone)]
enum Node {
    /// Free text outside any component marker (always present between/around
    /// components, possibly empty, to keep ours/theirs node sequences aligned).
    Interstitial(String),
    /// A full `<!-- agent:name -->...<!-- /agent:name -->` region, markers included.
    Component { name: String, text: String },
}

impl Node {
    fn component_name(&self) -> Option<&str> {
        match self {
            Node::Component { name, .. } => Some(name.as_str()),
            Node::Interstitial(_) => None,
        }
    }

    fn text(&self) -> &str {
        match self {
            Node::Interstitial(t) => t,
            Node::Component { text, .. } => text,
        }
    }
}

/// Segment `text` into top-level nodes: `I0 C0 I1 C1 … C(n-1) In`.
///
/// Interstitial slots are always emitted (even when empty) so two documents
/// with the same component-name sequence produce structurally-aligned node
/// vectors that can be paired by index.
fn segment_into_nodes(text: &str) -> Result<Vec<Node>> {
    let comps = crate::component::parse(text)?;
    // Top-level components only: a component is top-level if it is not nested
    // inside any other component's span.
    let mut top: Vec<&crate::component::Component> = comps
        .iter()
        .filter(|c| {
            !comps.iter().any(|o| {
                !std::ptr::eq(o, *c) && o.open_start <= c.open_start && c.close_end <= o.close_end
            })
        })
        .collect();
    top.sort_by_key(|c| c.open_start);

    let mut nodes = Vec::with_capacity(top.len() * 2 + 1);
    let mut cursor = 0usize;
    for c in top {
        // Interstitial slot before this component (possibly empty).
        nodes.push(Node::Interstitial(text[cursor..c.open_start].to_string()));
        nodes.push(Node::Component {
            name: c.name.clone(),
            text: text[c.open_start..c.close_end].to_string(),
        });
        cursor = c.close_end;
    }
    // Trailing interstitial slot (possibly empty).
    nodes.push(Node::Interstitial(text[cursor..].to_string()));
    Ok(nodes)
}

/// Component-scoped three-way CRDT merge (`#qcellmerge1`) — the anti-corruption rung.
///
/// Instead of reconciling the whole document as one text blob (which lets
/// unrelated edits contend and content splice across components — e.g. agent
/// console output merged INTO `agent:queue`), this splits ours/theirs/base into
/// per-component nodes and runs [`merge`] on each node against its own base.
///
/// - Nodes are paired by their component-name sequence. Component base text is
///   resolved by name (components are unique per document in practice), so the
///   `exchange` committed-response guard inside [`merge`] still sees its real base.
/// - **Structural divergence** — when ours and theirs disagree on which
///   components exist (a component added/removed on one side) — falls back to the
///   whole-document [`merge`] for the entire document, logged, never silently.
/// - A document with no `agent:*` components (inline mode) delegates directly to
///   [`merge`] with the original base state, preserving legacy behavior exactly.
pub fn merge_by_component(
    base_state: Option<&[u8]>,
    ours_text: &str,
    theirs_text: &str,
) -> Result<String> {
    if ours_text == theirs_text {
        return Ok(ours_text.to_string());
    }

    let ours_nodes = match segment_into_nodes(ours_text) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[crdt] merge_by_component: failed to segment ours ({e}); falling back to whole-doc merge"
            );
            return merge(base_state, ours_text, theirs_text);
        }
    };
    let theirs_nodes = match segment_into_nodes(theirs_text) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[crdt] merge_by_component: failed to segment theirs ({e}); falling back to whole-doc merge"
            );
            return merge(base_state, ours_text, theirs_text);
        }
    };

    let ours_names: Vec<&str> = ours_nodes.iter().filter_map(Node::component_name).collect();
    let theirs_names: Vec<&str> = theirs_nodes
        .iter()
        .filter_map(Node::component_name)
        .collect();

    // No components on either side (inline-mode doc): preserve exact legacy
    // behavior by delegating to the whole-doc merge with the original state.
    if ours_names.is_empty() && theirs_names.is_empty() {
        return merge(base_state, ours_text, theirs_text);
    }

    // Structural divergence: the set/order of components differs. A per-node
    // pairing is unsound, so fall back to the whole-doc merge (logged).
    if ours_names != theirs_names {
        eprintln!(
            "[crdt] merge_by_component: structural divergence (ours components {ours_names:?} != theirs {theirs_names:?}); falling back to whole-doc merge"
        );
        return merge(base_state, ours_text, theirs_text);
    }

    // Resolve base text and a name→content map for per-node base alignment.
    let base_text = match base_state {
        Some(bytes) => match CrdtDoc::decode_state(bytes) {
            Ok(doc) => doc.to_text(),
            Err(e) => {
                eprintln!(
                    "[crdt] merge_by_component: failed to decode base state ({e}); merging nodes without a base"
                );
                String::new()
            }
        },
        None => String::new(),
    };
    let base_nodes = segment_into_nodes(&base_text).unwrap_or_default();
    let mut base_by_name: std::collections::HashMap<&str, &str> =
        std::collections::HashMap::new();
    for node in &base_nodes {
        if let Node::Component { name, text } = node {
            base_by_name.entry(name.as_str()).or_insert(text.as_str());
        }
    }
    // Interstitial base slots, paired positionally with ours/theirs interstitials.
    let base_interstitials: Vec<&str> = base_nodes
        .iter()
        .filter_map(|c| match c {
            Node::Interstitial(t) => Some(t.as_str()),
            Node::Component { .. } => None,
        })
        .collect();

    let merged_nodes = merge_aligned_nodes(&ours_nodes, &theirs_nodes, |name, idx| match name {
        Some(n) => base_by_name.get(n).map(|s| s.to_string()),
        None => base_interstitials.get(idx).map(|s| s.to_string()),
    })?;
    Ok(merged_nodes.into_iter().map(|(_, text)| text).collect())
}

/// Merge aligned `ours`/`theirs` node vectors (same component-name sequence),
/// resolving each node's base text via `resolve_base`. `resolve_base` is called
/// with `(component_name, interstitial_index)` — `name` is `Some` for a
/// component node (and `interstitial_index` is meaningless), `None` for an
/// interstitial node (paired positionally by `interstitial_index`). Returns the
/// merged `(component_name, text)` per node in document order.
///
/// This is the shared per-node reconciliation loop behind both
/// [`merge_by_component`] (base resolved from a decoded whole-doc state) and
/// [`MultiNodeState::merge`] (base resolved from per-node persisted states).
fn merge_aligned_nodes<F>(
    ours_nodes: &[Node],
    theirs_nodes: &[Node],
    mut resolve_base: F,
) -> Result<Vec<(Option<String>, String)>>
where
    F: FnMut(Option<&str>, usize) -> Option<String>,
{
    let mut merged = Vec::with_capacity(ours_nodes.len());
    let mut interstitial_idx = 0usize;
    for (ours_node, theirs_node) in ours_nodes.iter().zip(theirs_nodes.iter()) {
        let name = ours_node.component_name();
        let node_base = resolve_base(name, interstitial_idx);
        if name.is_none() {
            interstitial_idx += 1;
        }

        let ours_slice = ours_node.text();
        let theirs_slice = theirs_node.text();

        let merged_text = if ours_slice == theirs_slice {
            ours_slice.to_string()
        } else {
            let node_base_state = node_base
                .as_deref()
                .map(|t| CrdtDoc::from_text(t).encode_state());
            merge(node_base_state.as_deref(), ours_slice, theirs_slice)?
        };
        merged.push((name.map(str::to_string), merged_text));
    }
    Ok(merged)
}

/// Encode `text` as a Yrs state with a **fixed** client id, so identical text
/// always produces byte-identical state. [`CrdtDoc::from_text`] uses
/// `Doc::new()`, which assigns a random client id, making every re-encode of the
/// same text differ — unsuitable for stable per-node persistence and no-change
/// detection. The base client id is irrelevant to [`merge`] (which reads only the
/// base text), so a fixed id is safe here.
fn encode_text_deterministic(text: &str) -> Vec<u8> {
    let doc = Doc::with_client_id(0);
    let yrs_text = doc.get_or_insert_text(TEXT_KEY);
    {
        let mut txn = doc.transact_mut();
        yrs_text.insert(&mut txn, 0, text);
    }
    let txn = doc.transact();
    txn.encode_state_as_update_v1(&yrs::StateVector::default())
}

/// Magic prefix identifying a [`MultiNodeState`] container on disk, so a per-node
/// state file (`<hash>.nodes.yrs`) is unambiguously distinguishable from a legacy
/// whole-doc Yrs update blob (`<hash>.yrs`), whose first bytes are a Yrs v1
/// update header rather than this ASCII tag.
const MULTINODE_MAGIC: &[u8; 4] = b"ADN1";

/// A document's CRDT merge state split into one independent Yrs state per
/// top-level node — the durable per-node base of `#qnodemerge2`.
///
/// The legacy whole-doc `<hash>.yrs` blob reconciles every node against one
/// shared base, so a node's base can never advance independently and unrelated
/// edits share a single Yrs clock. `MultiNodeState` instead persists one Yrs
/// state per node (each `agent:*` component plus the interstitial text around
/// them) in document order, serialized into a single structured container file
/// (`<hash>.nodes.yrs`). Each node therefore carries its own stable base across
/// cycles: a node untouched this cycle re-encodes to byte-identical state while a
/// changed node's state advances on its own, with no cross-node contention.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MultiNodeState {
    nodes: Vec<PersistedNode>,
}

/// One persisted node within a [`MultiNodeState`], in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PersistedNode {
    /// `Some(name)` for an `agent:*` component node; `None` for interstitial text.
    name: Option<String>,
    /// This node's own independent Yrs state (a [`CrdtDoc`] encoded state).
    state: Vec<u8>,
}

impl MultiNodeState {
    /// Build a per-node state by segmenting `text` into top-level nodes and
    /// encoding each node's text as its own independent Yrs state.
    pub fn from_text(text: &str) -> Result<Self> {
        let nodes = segment_into_nodes(text)?
            .iter()
            .map(|node| PersistedNode {
                name: node.component_name().map(str::to_string),
                state: encode_text_deterministic(node.text()),
            })
            .collect();
        Ok(MultiNodeState { nodes })
    }

    /// Build a per-node state directly from already-merged `(name, text)` nodes.
    fn from_merged_nodes(merged: &[(Option<String>, String)]) -> Self {
        MultiNodeState {
            nodes: merged
                .iter()
                .map(|(name, text)| PersistedNode {
                    name: name.clone(),
                    state: encode_text_deterministic(text),
                })
                .collect(),
        }
    }

    /// Decode a single node's persisted Yrs state back to its text.
    fn node_text(node: &PersistedNode) -> Result<String> {
        Ok(CrdtDoc::decode_state(&node.state)?.to_text())
    }

    /// Reconstruct the full document text by concatenating node texts in order.
    pub fn to_text(&self) -> Result<String> {
        let mut text = String::new();
        for node in &self.nodes {
            text.push_str(&Self::node_text(node)?);
        }
        Ok(text)
    }

    /// Resolve this state's per-node base into a `name → text` map (components)
    /// and an ordered list of interstitial texts, mirroring the alignment
    /// [`merge_aligned_nodes`] expects.
    fn base_lookup(
        &self,
    ) -> Result<(std::collections::HashMap<String, String>, Vec<String>)> {
        let mut by_name = std::collections::HashMap::new();
        let mut interstitials = Vec::new();
        for node in &self.nodes {
            let text = Self::node_text(node)?;
            match &node.name {
                Some(name) => {
                    by_name.entry(name.clone()).or_insert(text);
                }
                None => interstitials.push(text),
            }
        }
        Ok((by_name, interstitials))
    }

    /// Serialize into the `<hash>.nodes.yrs` container format:
    /// `MAGIC(4) | version(1) | node_count(u32 LE)` then per node
    /// `name_len(u32 LE) | name bytes | state_len(u32 LE) | state bytes`,
    /// where `name_len == u32::MAX` marks an interstitial node (no name).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MULTINODE_MAGIC);
        out.push(1u8); // container version
        out.extend_from_slice(&(self.nodes.len() as u32).to_le_bytes());
        for node in &self.nodes {
            match &node.name {
                Some(name) => {
                    let bytes = name.as_bytes();
                    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                    out.extend_from_slice(bytes);
                }
                None => out.extend_from_slice(&u32::MAX.to_le_bytes()),
            }
            out.extend_from_slice(&(node.state.len() as u32).to_le_bytes());
            out.extend_from_slice(&node.state);
        }
        out
    }

    /// Decode a `<hash>.nodes.yrs` container. Returns an error (never a panic) on
    /// a missing magic tag or any truncation, so callers can fall back to
    /// migration via [`MultiNodeState::decode_or_migrate`].
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut cursor = 0usize;
        let take = |cursor: &mut usize, n: usize| -> Result<&[u8]> {
            let end = cursor
                .checked_add(n)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| anyhow::anyhow!("multinode state truncated"))?;
            let slice = &bytes[*cursor..end];
            *cursor = end;
            Ok(slice)
        };
        let read_u32 = |cursor: &mut usize| -> Result<u32> {
            let b = take(cursor, 4)?;
            Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };

        if take(&mut cursor, 4)? != MULTINODE_MAGIC {
            anyhow::bail!("not a multinode state container (magic mismatch)");
        }
        let version = take(&mut cursor, 1)?[0];
        if version != 1 {
            anyhow::bail!("unsupported multinode state version {version}");
        }
        let count = read_u32(&mut cursor)? as usize;
        let mut nodes = Vec::with_capacity(count);
        for _ in 0..count {
            let name_len = read_u32(&mut cursor)?;
            let name = if name_len == u32::MAX {
                None
            } else {
                let bytes = take(&mut cursor, name_len as usize)?;
                Some(
                    std::str::from_utf8(bytes)
                        .map_err(|e| anyhow::anyhow!("invalid node name UTF-8: {e}"))?
                        .to_string(),
                )
            };
            let state_len = read_u32(&mut cursor)? as usize;
            let state = take(&mut cursor, state_len)?.to_vec();
            nodes.push(PersistedNode { name, state });
        }
        Ok(MultiNodeState { nodes })
    }

    /// Decode `bytes` as a per-node container, migrating older formats:
    /// 1. A `<hash>.nodes.yrs` container decodes directly.
    /// 2. Otherwise treat `bytes` as a legacy whole-doc `<hash>.yrs` Yrs state,
    ///    decode it to text, and split into per-node states.
    /// 3. If neither works, fall back to `fallback_text`.
    ///
    /// Never panics; logs the migration path it took.
    pub fn decode_or_migrate(bytes: &[u8], fallback_text: &str) -> Self {
        match Self::decode(bytes) {
            Ok(state) => return state,
            Err(_) => {
                // Not a per-node container — try the legacy whole-doc blob.
                if let Ok(doc) = CrdtDoc::decode_state(bytes)
                    && let Ok(state) = Self::from_text(&doc.to_text())
                {
                    eprintln!(
                        "[crdt] MultiNodeState: migrated legacy whole-doc state into per-node state"
                    );
                    return state;
                }
            }
        }
        eprintln!(
            "[crdt] MultiNodeState: state unreadable; rebuilding per-node state from fallback text"
        );
        Self::from_text(fallback_text).unwrap_or_default()
    }

    /// Per-node component-scoped three-way merge (`#qnodemerge2`).
    ///
    /// Like [`merge_by_component`] but each node reconciles against **its own
    /// persisted base** (carried in `base`) rather than a slice of one decoded
    /// whole-doc base, and the merged result is returned as a fresh
    /// `MultiNodeState` where each node's base has advanced independently. An
    /// unchanged node re-encodes to byte-identical state.
    ///
    /// Falls back to the whole-document [`merge`] (against `base`'s projected
    /// text) on structural divergence or a component-less document, preserving
    /// the same safety net as [`merge_by_component`].
    pub fn merge(
        base: Option<&MultiNodeState>,
        ours_text: &str,
        theirs_text: &str,
    ) -> Result<(String, MultiNodeState)> {
        let whole_doc_fallback = |reason: &str| -> Result<(String, MultiNodeState)> {
            let base_text = match base {
                Some(b) => b.to_text().unwrap_or_default(),
                None => String::new(),
            };
            let base_state = base.map(|_| CrdtDoc::from_text(&base_text).encode_state());
            if !reason.is_empty() {
                eprintln!("[crdt] MultiNodeState::merge: {reason}; falling back to whole-doc merge");
            }
            let merged = merge(base_state.as_deref(), ours_text, theirs_text)?;
            let state = MultiNodeState::from_text(&merged)?;
            Ok((merged, state))
        };

        if ours_text == theirs_text {
            return Ok((ours_text.to_string(), MultiNodeState::from_text(ours_text)?));
        }

        let ours_nodes = match segment_into_nodes(ours_text) {
            Ok(c) => c,
            Err(e) => return whole_doc_fallback(&format!("failed to segment ours ({e})")),
        };
        let theirs_nodes = match segment_into_nodes(theirs_text) {
            Ok(c) => c,
            Err(e) => return whole_doc_fallback(&format!("failed to segment theirs ({e})")),
        };

        let ours_names: Vec<&str> = ours_nodes.iter().filter_map(Node::component_name).collect();
        let theirs_names: Vec<&str> = theirs_nodes
            .iter()
            .filter_map(Node::component_name)
            .collect();

        if ours_names.is_empty() && theirs_names.is_empty() {
            return whole_doc_fallback("");
        }
        if ours_names != theirs_names {
            return whole_doc_fallback(&format!(
                "structural divergence (ours {ours_names:?} != theirs {theirs_names:?})"
            ));
        }

        let (base_by_name, base_interstitials) = match base {
            Some(b) => b.base_lookup()?,
            None => (std::collections::HashMap::new(), Vec::new()),
        };

        let merged_nodes = merge_aligned_nodes(&ours_nodes, &theirs_nodes, |name, idx| match name {
            Some(n) => base_by_name.get(n).cloned(),
            None => base_interstitials.get(idx).cloned(),
        })?;

        let merged_text: String = merged_nodes.iter().map(|(_, text)| text.as_str()).collect();
        let new_state = MultiNodeState::from_merged_nodes(&merged_nodes);
        Ok((merged_text, new_state))
    }
}

/// True for transient/structural lines that must not count as "new content"
/// introduced by `theirs` (boundary markers carry per-cycle ids and `(HEAD)`
/// annotations are working-tree-only artifacts).
fn is_transient_merge_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("<!-- agent:boundary:")
}

/// Collect normalized `### Re:` heading lines from a document, ignoring the
/// working-tree-only ` (HEAD)` annotation and surrounding whitespace. Matches
/// how the rest of the binary treats committed response headings.
fn response_headings(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| l.starts_with("### Re:"))
        .map(|l| {
            l.strip_suffix(" (HEAD)")
                .unwrap_or(l)
                .trim_end()
                .to_string()
        })
        .collect()
}

/// Restore committed response history when a stale/divergent `theirs` deleted
/// it. If the merge dropped a `### Re:` heading that exists in both `base` and
/// `ours`, and `theirs` contributed no new non-transient content beyond what
/// `ours` already has (i.e. `theirs` is a stale subset that only *lost*
/// committed content), the foreign view is stale — return `ours` so the
/// committed response is preserved. If `theirs` did introduce genuinely new
/// content, keep the merged result (never drop real user input) but log a
/// forensic warning. (#ipc-crdt-response-drift)
fn guard_committed_responses(
    merged: String,
    committed_response_headings: &[String],
    ours_text: &str,
    theirs_text: &str,
) -> String {
    if committed_response_headings.is_empty() {
        return merged;
    }
    let ours_headings = response_headings(ours_text);
    let merged_headings = response_headings(&merged);
    let lost: Vec<&String> = committed_response_headings
        .iter()
        .filter(|h| ours_headings.contains(h) && !merged_headings.contains(h))
        .collect();
    if lost.is_empty() {
        return merged;
    }

    // Does `theirs` introduce any new non-empty, non-transient line that `ours`
    // does not already contain? If so, it carries real user input we must keep.
    let ours_lines: std::collections::HashSet<&str> = ours_text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let theirs_adds_new = theirs_text
        .lines()
        .map(str::trim)
        .any(|l| !l.is_empty() && !is_transient_merge_line(l) && !ours_lines.contains(l));

    if theirs_adds_new {
        eprintln!(
            "[crdt] WARNING: merge dropped committed response heading(s) {:?} while theirs added new content; keeping merge (#ipc-crdt-response-drift)",
            lost
        );
        merged
    } else {
        eprintln!(
            "[crdt] stale theirs dropped committed response heading(s) {:?}; preferring ours (#ipc-crdt-response-drift)",
            lost
        );
        ours_text.to_string()
    }
}

/// Remove identical adjacent text blocks separated by blank lines.
///
/// After a CRDT merge, both sides may independently append the same content
/// (e.g., a `### Re:` section), resulting in duplicate adjacent blocks.
/// This pass deduplicates blocks with ≥1 non-empty line, exempting structural
/// blocks (thematic breaks like `---`) that may legitimately repeat.
pub fn dedup_adjacent_blocks(text: &str) -> String {
    let blocks: Vec<&str> = text.split("\n\n").collect();
    if blocks.len() < 2 {
        return text.to_string();
    }

    let mut result: Vec<&str> = Vec::with_capacity(blocks.len());
    for block in &blocks {
        let trimmed = block.trim();
        let non_empty_lines = trimmed.lines().filter(|l| !l.trim().is_empty()).count();
        if non_empty_lines >= 1
            && !is_structural_block(trimmed)
            && let Some(prev) = result.last()
            && prev.trim() == trimmed
        {
            eprintln!(
                "[crdt] dedup: removed duplicate block ({} lines)",
                non_empty_lines
            );
            continue;
        }
        result.push(*block);
    }

    result.join("\n\n")
}

/// Returns true for blocks that may legitimately repeat adjacent to themselves.
/// Currently covers thematic break markers (`---`, `***`, `___`).
fn is_structural_block(trimmed: &str) -> bool {
    let mut lines = trimmed.lines();
    let first = match lines.next() {
        Some(l) => l.trim(),
        None => return false,
    };
    if lines.next().is_some() {
        return false;
    }
    first.len() >= 3 && first.chars().all(|c| c == '-' || c == '*' || c == '_')
}

/// Compact a CRDT state by re-encoding (GC tombstones where possible).
pub fn compact(state: &[u8]) -> Result<Vec<u8>> {
    let doc = CrdtDoc::decode_state(state)?;
    Ok(doc.encode_state())
}

/// Count the number of bytes in the common prefix of two strings.
fn common_prefix_len(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

/// Count the number of bytes in the common suffix of two strings.
fn common_suffix_len(a: &str, b: &str) -> usize {
    a.bytes()
        .rev()
        .zip(b.bytes().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

/// Edit operation for replaying diffs onto a CRDT text.
#[derive(Debug)]
enum EditOp {
    Retain(u32),
    Delete(u32),
    Insert(String),
}

/// Compute edit operations to transform `from` into `to` using `similar` diff.
fn compute_edit_ops(from: &str, to: &str) -> Vec<EditOp> {
    use similar::{ChangeTag, TextDiff};

    let diff = TextDiff::from_lines(from, to);
    let mut ops = Vec::new();

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                let len = change.value().len() as u32;
                if let Some(EditOp::Retain(n)) = ops.last_mut() {
                    *n += len;
                } else {
                    ops.push(EditOp::Retain(len));
                }
            }
            ChangeTag::Delete => {
                let len = change.value().len() as u32;
                if let Some(EditOp::Delete(n)) = ops.last_mut() {
                    *n += len;
                } else {
                    ops.push(EditOp::Delete(len));
                }
            }
            ChangeTag::Insert => {
                let s = change.value().to_string();
                if let Some(EditOp::Insert(existing)) = ops.last_mut() {
                    existing.push_str(&s);
                } else {
                    ops.push(EditOp::Insert(s));
                }
            }
        }
    }

    ops
}

/// Apply edit operations to a Yrs text type within a transaction.
fn apply_ops(text: &TextRef, txn: &mut yrs::TransactionMut<'_>, ops: &[EditOp]) {
    let mut cursor: u32 = 0;
    for op in ops {
        match op {
            EditOp::Retain(n) => cursor += n,
            EditOp::Delete(n) => {
                text.remove_range(txn, cursor, *n);
                // cursor stays — content shifted left
            }
            EditOp::Insert(s) => {
                text.insert(txn, cursor, s);
                cursor += s.len() as u32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_text() {
        let content = "Hello, world!\nLine two.\n";
        let doc = CrdtDoc::from_text(content);
        assert_eq!(doc.to_text(), content);
    }

    #[test]
    fn roundtrip_encode_decode() {
        let content = "Some document content.\n";
        let doc = CrdtDoc::from_text(content);
        let encoded = doc.encode_state();
        let decoded = CrdtDoc::decode_state(&encoded).unwrap();
        assert_eq!(decoded.to_text(), content);
    }

    #[test]
    fn apply_edit_insert() {
        let doc = CrdtDoc::from_text("Hello world");
        doc.apply_edit(5, 0, ",");
        assert_eq!(doc.to_text(), "Hello, world");
    }

    #[test]
    fn apply_edit_delete() {
        let doc = CrdtDoc::from_text("Hello, world");
        doc.apply_edit(5, 1, "");
        assert_eq!(doc.to_text(), "Hello world");
    }

    #[test]
    fn apply_edit_replace() {
        let doc = CrdtDoc::from_text("Hello world");
        doc.apply_edit(6, 5, "Rust");
        assert_eq!(doc.to_text(), "Hello Rust");
    }

    #[test]
    fn concurrent_append_merge_no_conflict() {
        let base = "# Document\n\nBase content.\n";
        let base_doc = CrdtDoc::from_text(base);
        let base_state = base_doc.encode_state();

        let ours = format!("{base}## Agent\n\nAgent response.\n");
        let theirs = format!("{base}## User\n\nUser addition.\n");

        let merged = merge(Some(&base_state), &ours, &theirs).unwrap();

        // Both additions should be present
        assert!(merged.contains("Agent response."), "missing agent text");
        assert!(merged.contains("User addition."), "missing user text");
        assert!(merged.contains("Base content."), "missing base text");
        // No conflict markers
        assert!(!merged.contains("<<<<<<<"));
        assert!(!merged.contains(">>>>>>>"));
    }

    #[test]
    fn concurrent_insert_same_position() {
        let base = "Line 1\nLine 3\n";
        let base_doc = CrdtDoc::from_text(base);
        let base_state = base_doc.encode_state();

        let ours = "Line 1\nAgent line\nLine 3\n";
        let theirs = "Line 1\nUser line\nLine 3\n";

        let merged = merge(Some(&base_state), ours, theirs).unwrap();

        // Both insertions preserved, no conflict
        assert!(merged.contains("Agent line"), "missing agent insertion");
        assert!(merged.contains("User line"), "missing user insertion");
        assert!(merged.contains("Line 1"), "missing line 1");
        assert!(merged.contains("Line 3"), "missing line 3");
    }

    #[test]
    fn merge_no_base_state() {
        // When no base state exists, bootstrap from empty
        let ours = "Agent wrote this.\n";
        let theirs = "User wrote this.\n";

        let merged = merge(None, ours, theirs).unwrap();

        assert!(merged.contains("Agent wrote this."));
        assert!(merged.contains("User wrote this."));
    }

    #[test]
    fn compact_preserves_content() {
        let doc = CrdtDoc::from_text("Hello");
        doc.apply_edit(5, 0, " world");
        doc.apply_edit(11, 0, "!");

        let state = doc.encode_state();
        let compacted = compact(&state).unwrap();
        let restored = CrdtDoc::decode_state(&compacted).unwrap();

        assert_eq!(restored.to_text(), "Hello world!");
        assert!(compacted.len() <= state.len());
    }

    #[test]
    fn compact_reduces_size_after_edits() {
        let doc = CrdtDoc::from_text("aaaa");
        // Many small edits to build up tombstones
        for i in 0..20 {
            let c = ((b'a' + (i % 26)) as char).to_string();
            doc.apply_edit(0, 1, &c);
        }
        let state = doc.encode_state();
        let compacted = compact(&state).unwrap();
        let restored = CrdtDoc::decode_state(&compacted).unwrap();
        assert_eq!(restored.to_text(), doc.to_text());
    }

    #[test]
    fn empty_document() {
        let doc = CrdtDoc::from_text("");
        assert_eq!(doc.to_text(), "");

        let encoded = doc.encode_state();
        let decoded = CrdtDoc::decode_state(&encoded).unwrap();
        assert_eq!(decoded.to_text(), "");
    }

    #[test]
    fn decode_invalid_bytes_errors() {
        let result = CrdtDoc::decode_state(&[0xff, 0xfe, 0xfd]);
        assert!(result.is_err());
    }

    #[test]
    fn merge_identical_texts() {
        let base = "Same content.\n";
        let base_doc = CrdtDoc::from_text(base);
        let state = base_doc.encode_state();

        let merged = merge(Some(&state), base, base).unwrap();
        assert_eq!(merged, base);
    }

    #[test]
    fn merge_one_side_unchanged() {
        let base = "Original.\n";
        let base_doc = CrdtDoc::from_text(base);
        let state = base_doc.encode_state();

        let ours = "Original.\nAgent added.\n";
        let merged = merge(Some(&state), ours, base).unwrap();
        assert_eq!(merged, ours);
    }

    /// Regression test: CRDT merge should not duplicate user prompt when both
    /// ours and theirs contain the same text added since the base state.
    ///
    /// Scenario (brookebrodack-dev.md duplication bug):
    /// 1. CRDT base = exchange content from a previous cycle (no user prompt)
    /// 2. User adds prompt to exchange → saved as baseline
    /// 3. Agent generates response, content_ours = baseline + response (has user prompt)
    /// 4. User makes a small edit during response generation → content_current (has user prompt too)
    /// 5. CRDT merge: both ours and theirs have the user prompt relative to stale base
    /// 6. BUG: user prompt appears twice in merged output
    #[test]
    fn merge_stale_base_no_duplicate_user_prompt() {
        // CRDT base from a previous cycle — does NOT have the user's current prompt
        let base_content = "\
## Assistant

Previous response content.

Committed and pushed.

";
        let base_doc = CrdtDoc::from_text(base_content);
        let base_state = base_doc.encode_state();

        // User adds prompt after base was saved
        let user_prompt = "\
Opening a video a shows video a.
Closing video a then opening video b start video b but video b is hidden.
Closing video b then reopening video b starts and shows video b. video b is visible.
";

        // content_ours: base + user prompt + agent response (from run_stream with full exchange)
        let ours = format!(
            "\
{}{}### Re: Close A → Open B still hidden

Added explicit height and visibility reset.

Committed and pushed.

",
            base_content, user_prompt
        );

        // content_current: base + user prompt + minor user edit (e.g., added a blank line)
        let theirs = format!(
            "\
{}{}
",
            base_content, user_prompt
        );

        let merged = merge(Some(&base_state), &ours, &theirs).unwrap();

        // User prompt should appear exactly ONCE
        let prompt_count = merged.matches("Opening a video a shows video a.").count();
        assert_eq!(
            prompt_count, 1,
            "User prompt duplicated! Appeared {} times in:\n{}",
            prompt_count, merged
        );

        // Agent response should be present
        assert!(
            merged.contains("### Re: Close A → Open B still hidden"),
            "Agent response missing from merge:\n{}",
            merged
        );
    }

    /// Regression test: When CRDT base is stale and both sides added the same text
    /// at the same position, the merge should not duplicate it.
    #[test]
    fn merge_stale_base_same_insertion_both_sides() {
        let base_content = "Line 1\nLine 2\n";
        let base_doc = CrdtDoc::from_text(base_content);
        let base_state = base_doc.encode_state();

        // Both sides added the same text (user prompt) + ours adds more
        let shared_addition = "User typed this.\n";
        let ours = format!("{}{}Agent response.\n", base_content, shared_addition);
        let theirs = format!("{}{}", base_content, shared_addition);

        let merged = merge(Some(&base_state), &ours, &theirs).unwrap();

        let count = merged.matches("User typed this.").count();
        assert_eq!(
            count, 1,
            "Shared text duplicated! Appeared {} times in:\n{}",
            count, merged
        );
        assert!(
            merged.contains("Agent response."),
            "Agent text missing:\n{}",
            merged
        );
    }

    /// Regression test: Character-level interleaving bug.
    ///
    /// When the user types in their editor while the agent is streaming,
    /// both sides insert text at the same position relative to the base.
    /// The CRDT base advancement logic used to snap to the shared prefix
    /// of ours/theirs, which could land mid-line on a shared formatting
    /// character (e.g., `*` from `*bold*` and `**bold**`). This caused
    /// the formatting character to be absorbed into the base, splitting
    /// it from the rest of the formatting sequence and producing garbled
    /// text like `*Soft-bristle brush only**` instead of
    /// `**Soft-bristle brush only**`.
    ///
    /// The fix: always snap the advanced base to a line boundary. If no
    /// suitable line boundary exists after the current base length, don't
    /// advance at all.
    #[test]
    fn merge_no_character_interleaving() {
        // Base: a document with some existing content
        let base = "# Doc\n\nPrevious content.\n\n";
        let base_doc = CrdtDoc::from_text(base);
        let base_state = base_doc.encode_state();

        // Agent adds a response
        let ours = "# Doc\n\nPrevious content.\n\n*Compacted. Content archived to*\n";
        // User types something in their editor at the same position
        let theirs = "# Doc\n\nPrevious content.\n\n**Soft-bristle brush only**\n";

        let merged = merge(Some(&base_state), ours, theirs).unwrap();

        // Both texts should be present as contiguous blocks, not interleaved
        assert!(
            merged.contains("*Compacted. Content archived to*"),
            "Agent text should be contiguous (not interleaved). Got:\n{}",
            merged
        );
        assert!(
            merged.contains("**Soft-bristle brush only**"),
            "User text should be contiguous (not interleaved). Got:\n{}",
            merged
        );
    }

    /// Regression test: Concurrent edits within the same line should not
    /// produce character-level interleaving.
    #[test]
    fn merge_concurrent_same_line_no_garbling() {
        let base = "Some base text\n";
        let base_doc = CrdtDoc::from_text(base);
        let base_state = base_doc.encode_state();

        // Both sides replace the line with different content
        let ours = "Agent wrote this line\n";
        let theirs = "User wrote different text\n";

        let merged = merge(Some(&base_state), ours, theirs).unwrap();

        // At least one side's text should appear contiguously
        let has_agent_contiguous = merged.contains("Agent wrote this line");
        let has_user_contiguous = merged.contains("User wrote different text");

        assert!(
            has_agent_contiguous || has_user_contiguous,
            "At least one side should have contiguous text (no char interleaving). Got:\n{}",
            merged
        );
    }

    /// Regression test: Replace-vs-append corruption (lazily-rs.md bug).
    ///
    /// Pattern:
    /// - CRDT base is from a previous cycle (old exchange content)
    /// - Agent replaces exchange content entirely (template replace mode)
    /// - User appends new prompt text to exchange during response generation
    /// - CRDT interleaves agent's new content with user's old + new text,
    ///   causing mid-word splits like "key de" + [user text] + "cisions"
    ///
    /// Root cause: stale CRDT base doesn't match either side well enough
    /// for prefix advancement, so the CRDT does a raw character-level merge
    /// of the exchange section, interleaving replace and append operations.
    ///
    /// Fix: use baseline (not stored CRDT state) as merge base, so both
    /// sides' diffs are computed from the exact content they diverged from.
    #[test]
    fn merge_replace_vs_append_no_interleaving() {
        // Full document structure (template mode)
        let header = "---\nagent_doc_format: template\n---\n\n# Title\n\n<!-- agent:exchange -->\n";
        let footer = "\n<!-- /agent:exchange -->\n";

        // Previous cycle's exchange content (what the CRDT state contains)
        let old_exchange = "\
### Committed, Pushed & Released

**project (v0.1.0):**
- Committed initial implementation
- Tagged v0.1.0 and pushed

Add a README.md to the project.
Also add AGENTS.md with a symlink CLAUDE.md

**sub-project:**
- Committed fix + SPEC.md
- Pushed to remote
";
        let stale_base = format!("{header}{old_exchange}{footer}");
        let stale_state = CrdtDoc::from_text(&stale_base).encode_state();

        // Baseline (what the file looked like when response generation started)
        // Same as stale_base in this case — no user edits between cycles
        let _baseline = stale_base.clone();

        // Ours: agent replaces exchange content (template replace mode applied)
        let agent_exchange = "\
### Done

Added to project and pushed:

- **README.md** — overview, usage, design notes
- **AGENTS.md** — architecture, key decisions, commands, related projects
- **CLAUDE.md** → symlink to AGENTS.md

All committed and pushed.
";
        let ours = format!("{header}{agent_exchange}{footer}");

        // Theirs: user inserted new prompt IN THE MIDDLE of the exchange section
        // (after the existing user prompt, before the sub-project sections)
        // This is the critical difference — insertion within the range that ours deletes
        let theirs_exchange = "\
### Committed, Pushed & Released

**project (v0.1.0):**
- Committed initial implementation
- Tagged v0.1.0 and pushed

Add a README.md to the project.
Also add AGENTS.md with a symlink CLAUDE.md

Please add tests.
Please comprehensively test adherence to the spec.

**sub-project:**
- Committed fix + SPEC.md
- Pushed to remote
";
        let theirs = format!("{header}{theirs_exchange}{footer}");

        // Using stale CRDT state (previous cycle) — this is what triggers the bug
        let merged = merge(Some(&stale_state), &ours, &theirs).unwrap();

        // Agent's replacement text should be contiguous (no interleaving)
        assert!(
            merged.contains(
                "- **AGENTS.md** — architecture, key decisions, commands, related projects"
            ),
            "Agent text garbled (mid-word split). Got:\n{}",
            merged
        );

        // User's addition should be preserved
        assert!(
            merged.contains("Please add tests."),
            "User addition missing. Got:\n{}",
            merged
        );

        // No fragments of old content mixed into agent's new content
        assert!(
            !merged.contains("key deAdd") && !merged.contains("key de\n"),
            "Old content interleaved into agent text. Got:\n{}",
            merged
        );
    }

    /// Same as merge_replace_vs_append_no_interleaving but using baseline
    /// as CRDT base instead of stale state. This is the fix verification.
    #[test]
    fn merge_replace_vs_append_with_baseline_base() {
        let header = "---\nagent_doc_format: template\n---\n\n# Title\n\n<!-- agent:exchange -->\n";
        let footer = "\n<!-- /agent:exchange -->\n";

        let old_exchange = "\
### Previous Response

Old content here.

Add a README.md to the project.
Also add AGENTS.md with a symlink CLAUDE.md
";
        let baseline = format!("{header}{old_exchange}{footer}");

        // Ours: agent replaces exchange
        let agent_exchange = "\
### Done

- **README.md** — overview, usage, design notes
- **AGENTS.md** — architecture, key decisions, commands, related projects
- **CLAUDE.md** → symlink to AGENTS.md

All committed and pushed.
";
        let ours = format!("{header}{agent_exchange}{footer}");

        // Theirs: user appended new prompt
        let user_addition = "\nPlease add tests.\n";
        let theirs = format!("{header}{old_exchange}{user_addition}{footer}");

        // Use baseline as CRDT base (the fix)
        let baseline_state = CrdtDoc::from_text(&baseline).encode_state();
        let merged = merge(Some(&baseline_state), &ours, &theirs).unwrap();

        // Agent text should be contiguous
        assert!(
            merged.contains("key decisions, commands, related projects"),
            "Agent text garbled. Got:\n{}",
            merged
        );

        // User addition preserved
        assert!(
            merged.contains("Please add tests."),
            "User addition missing. Got:\n{}",
            merged
        );
    }

    /// Regression test: Simulates the exact scenario from the bug report.
    ///
    /// The agent streams a response into the exchange component while
    /// the user types in their editor. Both sides share a common prefix
    /// that includes markdown formatting characters. The CRDT merge must
    /// preserve formatting integrity for both sides.
    #[test]
    fn merge_streaming_concurrent_edit_preserves_formatting() {
        // Exchange component content after user's initial prompt
        let base = "commit and push all rappstack packages.\n\n";
        let base_doc = CrdtDoc::from_text(base);
        let base_state = base_doc.encode_state();

        // Agent's response (content_ours = user prompt + agent response)
        let ours = "\
commit and push all rappstack packages.

### Re: commit and push

*Compacted. Content archived to `docs/`*

Done — all packages pushed.
";

        // User's concurrent edit (added a note at the bottom)
        let theirs = "\
commit and push all rappstack packages.

**Soft-bristle brush only**
";

        let merged = merge(Some(&base_state), ours, theirs).unwrap();

        // Agent formatting must be intact
        assert!(
            merged.contains("*Compacted. Content archived to `docs/`*"),
            "Agent formatting broken. Got:\n{}",
            merged
        );
        // User formatting must be intact
        assert!(
            merged.contains("**Soft-bristle brush only**"),
            "User formatting broken. Got:\n{}",
            merged
        );
        // No character-level interleaving
        assert!(
            !merged.contains("*C*C") && !merged.contains("**Sot"),
            "Character interleaving detected. Got:\n{}",
            merged
        );
    }

    /// Regression test: Agent replaces multi-line block while user inserts within it.
    /// With from_chars, this produces ~20 scattered character-level ops that interleave
    /// with user edits. With from_lines, ops are contiguous line-level blocks.
    ///
    /// Uses a template document structure to match the real workflow where the baseline
    /// (common ancestor) contains the exchange component with original content.
    #[test]
    fn merge_replace_vs_insert_no_interleaving() {
        let header = "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n# Document Title\n\nSome preamble text that both sides share.\nThis provides enough common prefix to avoid stale detection.\n\n<!-- agent:exchange -->\n";
        let footer = "<!-- /agent:exchange -->\n";

        let old_exchange =
            "Line one of old content\nLine two of old content\nLine three of old content\n";
        let baseline = format!("{header}{old_exchange}{footer}");
        let baseline_doc = CrdtDoc::from_text(&baseline);
        let baseline_state = baseline_doc.encode_state();

        // Agent replaces exchange with completely new content
        let agent_exchange = "Completely new line one\nCompletely new line two\nCompletely new line three\nCompletely new line four\n";
        let ours = format!("{header}{agent_exchange}{footer}");

        // User inserts a line in the middle of the original exchange
        let theirs = format!(
            "{header}Line one of old content\nUser inserted this line\nLine two of old content\nLine three of old content\n{footer}"
        );

        let merged = merge(Some(&baseline_state), &ours, &theirs).unwrap();

        // Agent text should be contiguous — no mid-word splits
        assert!(
            merged.contains("Completely new line one"),
            "Agent line 1 missing or garbled. Got:\n{}",
            merged
        );
        assert!(
            merged.contains("Completely new line two"),
            "Agent line 2 missing or garbled. Got:\n{}",
            merged
        );

        // User text should be preserved
        assert!(
            merged.contains("User inserted this line"),
            "User insertion missing. Got:\n{}",
            merged
        );

        // No character interleaving (e.g., "Complete" + user text + "ly")
        assert!(
            !merged.contains("CompleteUser") && !merged.contains("Complete\nUser"),
            "Character interleaving detected. Got:\n{}",
            merged
        );
    }

    /// Test: agent content appears before human content when both append
    /// to the same position.
    #[test]
    fn reorder_agent_before_human_at_append_boundary() {
        let base = "# Document\n\nBase content.\n";
        let base_doc = CrdtDoc::from_text(base);
        let base_state = base_doc.encode_state();

        // Agent appends response
        let ours = format!("{base}### Agent Response\n\nAgent wrote this.\n");
        // Human appends their own text
        let theirs = format!("{base}User added this line.\n");

        let merged = merge(Some(&base_state), &ours, &theirs).unwrap();

        // Both should be present
        assert!(merged.contains("Agent wrote this."), "missing agent text");
        assert!(
            merged.contains("User added this line."),
            "missing user text"
        );
        assert!(merged.contains("Base content."), "missing base text");

        // Agent content should appear before human content
        let agent_pos = merged.find("Agent wrote this.").unwrap();
        let human_pos = merged.find("User added this line.").unwrap();
        assert!(
            agent_pos < human_pos,
            "Agent content should appear before human content.\nAgent pos: {}, Human pos: {}\nMerged:\n{}",
            agent_pos,
            human_pos,
            merged
        );
    }

    // -----------------------------------------------------------------------
    // dedup_adjacent_blocks tests (#15)
    // -----------------------------------------------------------------------

    #[test]
    fn dedup_removes_identical_adjacent_blocks() {
        let text =
            "### Re: Question\nAnswer here.\n\n### Re: Question\nAnswer here.\n\nDifferent block.";
        let result = dedup_adjacent_blocks(text);
        assert_eq!(result.matches("### Re: Question").count(), 1);
        assert!(result.contains("Different block."));
    }

    #[test]
    fn dedup_preserves_different_adjacent_blocks() {
        let text = "### Re: First\nAnswer one.\n\n### Re: Second\nAnswer two.";
        let result = dedup_adjacent_blocks(text);
        assert!(result.contains("### Re: First"));
        assert!(result.contains("### Re: Second"));
    }

    #[test]
    fn dedup_preserves_structural_thematic_breaks() {
        let text = "---\n\n---\n\nContent.";
        let result = dedup_adjacent_blocks(text);
        assert_eq!(result, text);

        let text2 = "***\n\n***\n\nContent.";
        assert_eq!(dedup_adjacent_blocks(text2), text2);

        let text3 = "___\n\n___\n\nContent.";
        assert_eq!(dedup_adjacent_blocks(text3), text3);
    }

    #[test]
    fn dedup_removes_single_line_content_duplicates() {
        let text = "### Re: topic — opus-4-6\n\n### Re: topic — opus-4-6\n\nDifferent.";
        let result = dedup_adjacent_blocks(text);
        assert_eq!(result.matches("### Re: topic").count(), 1);
        assert!(result.contains("Different."));
    }

    #[test]
    fn dedup_removes_single_line_text_duplicates() {
        let text = "Some response text.\n\nSome response text.\n\nNext section.";
        let result = dedup_adjacent_blocks(text);
        assert_eq!(result.matches("Some response text.").count(), 1);
        assert!(result.contains("Next section."));
    }

    #[test]
    fn dedup_preserves_different_single_line_blocks() {
        let text = "Line A.\n\nLine B.\n\nLine C.";
        let result = dedup_adjacent_blocks(text);
        assert_eq!(result, text);
    }

    #[test]
    fn dedup_handles_empty_text() {
        assert_eq!(dedup_adjacent_blocks(""), "");
    }

    #[test]
    fn dedup_no_change_when_no_duplicates() {
        let text = "Block A\nLine 2.\n\nBlock B\nLine 2.";
        let result = dedup_adjacent_blocks(text);
        assert_eq!(result, text);
    }

    #[test]
    fn is_structural_block_detects_thematic_breaks() {
        assert!(is_structural_block("---"));
        assert!(is_structural_block("***"));
        assert!(is_structural_block("___"));
        assert!(is_structural_block("-----"));
        assert!(!is_structural_block("--"));
        assert!(!is_structural_block("### Heading"));
        assert!(!is_structural_block("some text"));
        assert!(!is_structural_block("---\nmore"));
    }

    /// Regression test: empty-baseline CRDT merge duplicates last user prompt line.
    ///
    /// Scenario (claude-code-action.md bug):
    /// 1. Baseline = template document with empty exchange (just boundary marker)
    /// 2. content_ours = baseline + response patches (response replaces boundary)
    /// 3. content_current = user's file (user typed prompt before boundary)
    /// 4. CRDT merge: both sides added content to the empty exchange
    /// 5. BUG: last line of user's prompt appears twice (before and after response)
    #[test]
    fn merge_empty_exchange_no_duplicate_user_line() {
        let header = "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n";
        let footer = "<!-- /agent:exchange -->\n\n## Pending\n\n<!-- agent:pending -->\n<!-- /agent:pending -->\n";

        // Base: empty exchange with boundary marker
        let base = format!("{header}\n<!-- agent:boundary:base-id -->\n{footer}");

        // Ours: response applied via boundary-aware append (boundary replaced)
        let ours = format!(
            "{header}\n### Re: topic — opus-4-6\n\nResponse content here.\n\nMore response text.\n\n<!-- agent:boundary:new-id -->\n{footer}"
        );

        // Theirs: user typed prompt before the boundary
        let theirs = format!(
            "{header}User prompt line one.\nUser prompt line two.\nLast line of user prompt.\n<!-- agent:boundary:base-id -->\n{footer}"
        );

        let base_doc = CrdtDoc::from_text(&base);
        let base_state = base_doc.encode_state();

        let merged = merge(Some(&base_state), &ours, &theirs).unwrap();

        // Each user prompt line should appear exactly once
        let count1 = merged.matches("User prompt line one.").count();
        assert_eq!(
            count1, 1,
            "Line one duplicated (count={}). Merged:\n{}",
            count1, merged
        );

        let count2 = merged.matches("User prompt line two.").count();
        assert_eq!(
            count2, 1,
            "Line two duplicated (count={}). Merged:\n{}",
            count2, merged
        );

        let count3 = merged.matches("Last line of user prompt.").count();
        assert_eq!(
            count3, 1,
            "Last line duplicated (count={}). Merged:\n{}",
            count3, merged
        );

        // Response should appear exactly once
        assert!(
            merged.contains("### Re: topic — opus-4-6"),
            "Response heading missing:\n{}",
            merged
        );
        assert!(
            merged.contains("Response content here."),
            "Response content missing:\n{}",
            merged
        );

        // User prompt should come BEFORE response
        let prompt_pos = merged.find("User prompt line one.").unwrap();
        let response_pos = merged.find("### Re: topic").unwrap();
        assert!(
            prompt_pos < response_pos,
            "User prompt should precede response. prompt={} response={}\nMerged:\n{}",
            prompt_pos,
            response_pos,
            merged
        );
    }

    /// Same as above but with ❯ prefixed user content (post-normalization).
    #[test]
    fn merge_empty_exchange_prefixed_prompts_no_duplicate() {
        let header = "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n";
        let footer = "<!-- /agent:exchange -->\n";

        let base = format!("{header}\n<!-- agent:boundary:base-id -->\n{footer}");

        // Ours: response + normalized ❯ prefix on user lines
        let ours = format!(
            "{header}❯ Review the code\n❯ What does it do?\n\n### Re: code review — opus-4-6\n\nThe code does X.\n\n<!-- agent:boundary:new-id -->\n{footer}"
        );

        // Theirs: user's file with same prompt (also ❯ prefixed by IPC normalization)
        let theirs = format!(
            "{header}❯ Review the code\n❯ What does it do?\n<!-- agent:boundary:base-id -->\n{footer}"
        );

        let base_doc = CrdtDoc::from_text(&base);
        let base_state = base_doc.encode_state();

        let merged = merge(Some(&base_state), &ours, &theirs).unwrap();

        let count = merged.matches("❯ Review the code").count();
        assert_eq!(
            count, 1,
            "Prompt duplicated (count={}). Merged:\n{}",
            count, merged
        );

        assert!(
            merged.contains("The code does X."),
            "Response missing:\n{}",
            merged
        );
    }

    /// Regression test (#ipc-crdt-response-drift): a concurrent **foreign**
    /// writer (another supervisor / queue toggle) appends to the document at
    /// the tail *during* response generation, growing `theirs` beyond `base`.
    /// The agent's new `### Re:` response (ours) must:
    ///   1. land with its heading intact (not dropped),
    ///   2. keep its lines in author order (not reversed),
    ///   3. appear AFTER the prior committed `### Re:` response (not hoisted
    ///      above it),
    ///   4. preserve the foreign tail append.
    #[test]
    fn merge_foreign_tail_append_preserves_response_order() {
        let header = "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n";
        let exchange_committed = "\
❯ first question

### Re: first question — opus-4-8

First answer here. Already committed to HEAD.

❯ second question
";
        let queue_open = "<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n";
        let queue_close = "<!-- /agent:queue -->\n";

        // Base: prior response committed, new prompt typed, boundary at tail of
        // exchange, empty queue.
        let base = format!(
            "{header}{exchange_committed}<!-- agent:boundary:base-id -->\n{queue_open}{queue_close}"
        );

        // Ours: agent replaces boundary with the new response (multi-line).
        let agent_response = "\
### Re: second question — opus-4-8

Second answer line one.
Second answer line two.
Second answer line three.

";
        let ours = format!(
            "{header}{exchange_committed}{agent_response}<!-- agent:boundary:new-id -->\n{queue_open}{queue_close}"
        );

        // Theirs: a foreign supervisor appended a queue item at the tail while
        // the response was being generated (file grew, base unchanged in body).
        let theirs = format!(
            "{header}{exchange_committed}<!-- agent:boundary:base-id -->\n{queue_open}do [#foreign-task]\n{queue_close}"
        );

        let base_state = CrdtDoc::from_text(&base).encode_state();
        let merged = merge(Some(&base_state), &ours, &theirs).unwrap();

        // 1. Heading intact.
        assert!(
            merged.contains("### Re: second question — opus-4-8"),
            "new response heading dropped:\n{}",
            merged
        );
        // 2. Lines in author order (not reversed).
        let l1 = merged
            .find("Second answer line one.")
            .expect("line one missing");
        let l2 = merged
            .find("Second answer line two.")
            .expect("line two missing");
        let l3 = merged
            .find("Second answer line three.")
            .expect("line three missing");
        assert!(
            l1 < l2 && l2 < l3,
            "response lines reversed/reordered (l1={l1} l2={l2} l3={l3}):\n{merged}"
        );
        // 3. New response appears AFTER the prior committed response.
        let prior = merged
            .find("### Re: first question")
            .expect("prior response missing");
        let current = merged.find("### Re: second question").unwrap();
        assert!(
            prior < current,
            "new response hoisted above prior HEAD response (prior={prior} current={current}):\n{merged}"
        );
        // 4. Foreign tail append preserved.
        assert!(
            merged.contains("do [#foreign-task]"),
            "foreign queue append lost:\n{}",
            merged
        );
    }

    /// Regression test (#ipc-crdt-response-drift): a stale/divergent `theirs`
    /// (e.g. a stale editor live-buffer pushed by convergence, or a concurrent
    /// foreign supervisor) presents a document that has *lost* the prior
    /// committed `### Re:` response block. The merge must NOT honor that foreign
    /// deletion — the committed response and its heading must survive.
    #[test]
    fn merge_stale_theirs_cannot_delete_committed_response() {
        let header = "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n";
        let committed = "❯ first question\n\n### Re: first question — opus-4-8\n\nFirst answer. Committed to HEAD.\n\n❯ second question\n";
        let foot = "<!-- /agent:exchange -->\n";
        let base = format!("{header}{committed}<!-- agent:boundary:base-id -->\n{foot}");
        let resp = "### Re: second question — opus-4-8\n\nLine one.\nLine two.\nLine three.\n\n";
        let ours = format!("{header}{committed}{resp}<!-- agent:boundary:new-id -->\n{foot}");

        // Theirs: stale buffer that dropped the committed first Q&A entirely
        // (only the boundary id differs from ours — a transient marker).
        let theirs = format!("{header}❯ second question\n<!-- agent:boundary:base-id -->\n{foot}");

        let base_state = CrdtDoc::from_text(&base).encode_state();
        let merged = merge(Some(&base_state), &ours, &theirs).unwrap();

        assert!(
            merged.contains("### Re: first question — opus-4-8"),
            "committed prior response heading deleted by stale theirs:\n{merged}"
        );
        assert!(
            merged.contains("First answer. Committed to HEAD."),
            "committed prior response body deleted by stale theirs:\n{merged}"
        );
        assert!(
            merged.contains("### Re: second question — opus-4-8"),
            "new response heading missing:\n{merged}"
        );
        // Order preserved: prior committed response before the new one.
        let prior = merged.find("### Re: first question").unwrap();
        let current = merged.find("### Re: second question").unwrap();
        assert!(prior < current, "response order inverted:\n{merged}");
    }

    /// Negative case for the committed-response guard: when `theirs` carries
    /// genuinely new user input (a freshly typed prompt) the guard must NOT
    /// clobber it by preferring `ours`; both the new user prompt and the
    /// committed history must survive.
    #[test]
    fn merge_committed_guard_keeps_new_user_input() {
        let header = "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n";
        let committed = "❯ first question\n\n### Re: first question — opus-4-8\n\nFirst answer. Committed to HEAD.\n\n❯ second question\n";
        let foot = "<!-- /agent:exchange -->\n";
        let base = format!("{header}{committed}<!-- agent:boundary:base-id -->\n{foot}");
        let resp = "### Re: second question — opus-4-8\n\nAnswer body.\n\n";
        let ours = format!("{header}{committed}{resp}<!-- agent:boundary:new-id -->\n{foot}");

        // Theirs: user appended a brand-new prompt while the response generated.
        let theirs = format!(
            "{header}{committed}❯ a genuinely new third prompt\n<!-- agent:boundary:base-id -->\n{foot}"
        );

        let base_state = CrdtDoc::from_text(&base).encode_state();
        let merged = merge(Some(&base_state), &ours, &theirs).unwrap();

        assert!(
            merged.contains("❯ a genuinely new third prompt"),
            "new user prompt clobbered by committed-response guard:\n{merged}"
        );
        assert!(
            merged.contains("### Re: first question"),
            "committed prior response lost:\n{merged}"
        );
        assert!(
            merged.contains("### Re: second question"),
            "new response lost:\n{merged}"
        );
    }

    // ---- #qcellmerge1: component-scoped merge -----------------------------

    /// Build a template doc with an `exchange` and a `queue` component.
    fn doc_with_exchange_queue(exchange: &str, queue: &str) -> String {
        format!(
            "---\nagent_doc_format: template\n---\n\n## Exchange\n\n<!-- agent:exchange -->\n{exchange}\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n{queue}\n<!-- /agent:queue -->\n"
        )
    }

    #[test]
    fn merge_by_component_isolates_concurrent_cross_component_edits() {
        // Side A (agent) appends to `exchange`; side B (operator) appends a
        // `queue` item. They must BOTH survive with ZERO cross-component splice.
        let base = doc_with_exchange_queue("Existing prompt.", "- do [#a1]");
        let base_state = CrdtDoc::from_text(&base).encode_state();

        let ours = doc_with_exchange_queue(
            "Existing prompt.\n\n### Re: existing — opus\n\nAgent response body.",
            "- do [#a1]",
        );
        let theirs = doc_with_exchange_queue("Existing prompt.", "- do [#a1]\n- do [#b2]");

        let merged = merge_by_component(Some(&base_state), &ours, &theirs).unwrap();

        // Both edits present.
        assert!(
            merged.contains("### Re: existing"),
            "agent exchange edit lost:\n{merged}"
        );
        assert!(merged.contains("[#b2]"), "operator queue edit lost:\n{merged}");

        // ZERO cross-component splice: the agent response body must not appear
        // inside the queue component, and the queue item must not appear inside
        // exchange.
        let queue_body = merged
            .split("<!-- agent:queue -->")
            .nth(1)
            .and_then(|s| s.split("<!-- /agent:queue -->").next())
            .unwrap_or("");
        let exchange_body = merged
            .split("<!-- agent:exchange -->")
            .nth(1)
            .and_then(|s| s.split("<!-- /agent:exchange -->").next())
            .unwrap_or("");
        assert!(
            !queue_body.contains("Agent response body"),
            "exchange content spliced into queue:\n{queue_body}"
        );
        assert!(
            !exchange_body.contains("[#b2]"),
            "queue content spliced into exchange:\n{exchange_body}"
        );
    }

    #[test]
    fn merge_by_component_blocks_console_output_in_queue_corruption() {
        // The live 2026-06-20 corruption: agent console output ("Supervisor is
        // now fresh…") merged INTO agent:queue as a fenced block while the
        // operator typed a queue prompt. Model it as: ours = agent wrote console
        // text into `exchange`, theirs = operator typed a queue prompt. The
        // console text must never land in the queue node.
        let console = "```\n● Supervisor is now fresh (recycled to 0.34.35).\n```";
        let base = doc_with_exchange_queue("Q.", "- :pushpin: typing here");
        let base_state = CrdtDoc::from_text(&base).encode_state();

        let ours = doc_with_exchange_queue(&format!("Q.\n\n### Re: q\n\n{console}"), "- :pushpin: typing here");
        let theirs = doc_with_exchange_queue("Q.", "- :pushpin: typing here now extended");

        let merged = merge_by_component(Some(&base_state), &ours, &theirs).unwrap();

        let queue_body = merged
            .split("<!-- agent:queue -->")
            .nth(1)
            .and_then(|s| s.split("<!-- /agent:queue -->").next())
            .unwrap_or("");
        assert!(
            !queue_body.contains("Supervisor is now fresh"),
            "console output corrupted the queue node:\n{queue_body}"
        );
        assert!(
            queue_body.contains("extended"),
            "operator queue edit lost:\n{queue_body}"
        );
        assert!(
            merged.contains("Supervisor is now fresh"),
            "console output must remain in its own (exchange) node:\n{merged}"
        );
    }

    #[test]
    fn merge_by_component_degenerates_to_whole_doc_without_components() {
        // Inline-mode docs (no agent:* components) must behave exactly like the
        // legacy whole-doc merge.
        let base = "# Document\n\nBase content.\n";
        let base_state = CrdtDoc::from_text(base).encode_state();
        let ours = format!("{base}## Agent\n\nAgent response.\n");
        let theirs = format!("{base}## User\n\nUser addition.\n");

        let by_component = merge_by_component(Some(&base_state), &ours, &theirs).unwrap();
        let whole_doc = merge(Some(&base_state), &ours, &theirs).unwrap();
        assert_eq!(
            by_component, whole_doc,
            "component merge must equal whole-doc merge for a component-less doc"
        );
    }

    #[test]
    fn merge_by_component_falls_back_on_structural_divergence() {
        // theirs removes the queue component entirely → structural divergence →
        // whole-doc fallback (no panic, content preserved deterministically).
        let base = doc_with_exchange_queue("Q.", "- do [#a1]");
        let base_state = CrdtDoc::from_text(&base).encode_state();
        let ours = doc_with_exchange_queue("Q.\n\n### Re: q\n\nResponse.", "- do [#a1]");
        // theirs: no queue component at all.
        let theirs = "---\nagent_doc_format: template\n---\n\n## Exchange\n\n<!-- agent:exchange -->\nQ.\n<!-- /agent:exchange -->\n".to_string();

        // Must not panic and must equal the whole-doc merge fallback.
        let by_component = merge_by_component(Some(&base_state), &ours, &theirs).unwrap();
        let whole_doc = merge(Some(&base_state), &ours, &theirs).unwrap();
        assert_eq!(by_component, whole_doc, "structural divergence must use whole-doc fallback");
    }

    // ---- #qnodemerge2: per-node CRDT state persistence --------------------

    #[test]
    fn multinode_state_roundtrip_encode_decode() {
        // Per-component encode/decode round-trips through the container format and
        // reconstructs the exact document text.
        let doc = doc_with_exchange_queue("Prompt.\n\n### Re: q\n\nBody.", "- do [#a1]\n- do [#b2]");
        let state = MultiNodeState::from_text(&doc).unwrap();
        let encoded = state.encode();
        let decoded = MultiNodeState::decode(&encoded).unwrap();
        assert_eq!(decoded, state, "decode must reproduce the encoded per-node state");
        assert_eq!(
            decoded.to_text().unwrap(),
            doc,
            "per-node state must reconstruct the original document text"
        );
        // The container is self-describing — one node per interstitial+component.
        assert!(decoded.nodes.len() >= 4, "expected ≥4 nodes, got {}", decoded.nodes.len());
        assert!(decoded.nodes.iter().any(|n| n.name.as_deref() == Some("exchange")));
        assert!(decoded.nodes.iter().any(|n| n.name.as_deref() == Some("queue")));
    }

    #[test]
    fn multinode_state_decode_rejects_non_container() {
        // A legacy whole-doc Yrs blob is not a per-node container — decode must
        // error (not panic) so the caller can migrate it.
        let legacy = CrdtDoc::from_text("# Doc\n\nlegacy whole-doc state\n").encode_state();
        assert!(
            MultiNodeState::decode(&legacy).is_err(),
            "legacy whole-doc state must not decode as a per-node container"
        );
    }

    #[test]
    fn multinode_state_migrates_legacy_whole_doc() {
        // decode_or_migrate must split a legacy whole-doc state into per-node states.
        let doc = doc_with_exchange_queue("Q.", "- do [#a1]");
        let legacy = CrdtDoc::from_text(&doc).encode_state();
        let migrated = MultiNodeState::decode_or_migrate(&legacy, "");
        assert_eq!(
            migrated.to_text().unwrap(),
            doc,
            "legacy migration must preserve the full document text"
        );
        assert!(
            migrated.nodes.iter().any(|n| n.name.as_deref() == Some("queue")),
            "legacy migration must recover component nodes"
        );
    }

    #[test]
    fn multinode_merge_advances_node_base_independently() {
        // The core #qnodemerge2 property: when only the `exchange` node changes,
        // the `queue` node's persisted base is byte-identical across the cycle,
        // while the `exchange` node's base advances.
        let base_doc = doc_with_exchange_queue("Q.", "- do [#a1]");
        let base = MultiNodeState::from_text(&base_doc).unwrap();

        // Agent appends to exchange; operator leaves queue untouched.
        let ours = doc_with_exchange_queue("Q.\n\n### Re: q\n\nAgent body.", "- do [#a1]");
        let theirs = base_doc.clone();

        let (merged, advanced) = MultiNodeState::merge(Some(&base), &ours, &theirs).unwrap();
        assert!(merged.contains("Agent body."), "exchange edit lost:\n{merged}");

        let queue_before = base
            .nodes
            .iter()
            .find(|n| n.name.as_deref() == Some("queue"))
            .map(|n| &n.state);
        let queue_after = advanced
            .nodes
            .iter()
            .find(|n| n.name.as_deref() == Some("queue"))
            .map(|n| &n.state);
        assert_eq!(
            queue_before, queue_after,
            "untouched queue node base must be byte-identical across the cycle"
        );

        let exchange_before = base
            .nodes
            .iter()
            .find(|n| n.name.as_deref() == Some("exchange"))
            .map(|n| &n.state);
        let exchange_after = advanced
            .nodes
            .iter()
            .find(|n| n.name.as_deref() == Some("exchange"))
            .map(|n| &n.state);
        assert_ne!(
            exchange_before, exchange_after,
            "changed exchange node base must advance"
        );
    }

    #[test]
    fn multinode_merge_isolates_concurrent_cross_component_edits() {
        // Parallel to merge_by_component: concurrent exchange + queue edits both
        // survive with zero cross-component splice, via per-node bases.
        let base_doc = doc_with_exchange_queue("Existing prompt.", "- do [#a1]");
        let base = MultiNodeState::from_text(&base_doc).unwrap();

        let ours = doc_with_exchange_queue(
            "Existing prompt.\n\n### Re: existing — opus\n\nAgent response body.",
            "- do [#a1]",
        );
        let theirs = doc_with_exchange_queue("Existing prompt.", "- do [#a1]\n- do [#b2]");

        let (merged, _advanced) = MultiNodeState::merge(Some(&base), &ours, &theirs).unwrap();
        assert!(merged.contains("### Re: existing"), "exchange edit lost:\n{merged}");
        assert!(merged.contains("[#b2]"), "queue edit lost:\n{merged}");

        let queue_body = merged
            .split("<!-- agent:queue -->")
            .nth(1)
            .and_then(|s| s.split("<!-- /agent:queue -->").next())
            .unwrap_or("");
        assert!(
            !queue_body.contains("Agent response body"),
            "exchange content spliced into queue:\n{queue_body}"
        );
    }

    #[test]
    fn multinode_merge_falls_back_on_structural_divergence() {
        // theirs drops the queue component → whole-doc fallback, still producing a
        // valid per-node state that round-trips the merged text.
        let base_doc = doc_with_exchange_queue("Q.", "- do [#a1]");
        let base = MultiNodeState::from_text(&base_doc).unwrap();
        let ours = doc_with_exchange_queue("Q.\n\n### Re: q\n\nResponse.", "- do [#a1]");
        let theirs = "---\nagent_doc_format: template\n---\n\n## Exchange\n\n<!-- agent:exchange -->\nQ.\n<!-- /agent:exchange -->\n".to_string();

        let (merged, advanced) = MultiNodeState::merge(Some(&base), &ours, &theirs).unwrap();
        let whole_doc = {
            let base_state = CrdtDoc::from_text(&base_doc).encode_state();
            merge(Some(&base_state), &ours, &theirs).unwrap()
        };
        assert_eq!(merged, whole_doc, "structural divergence must use whole-doc fallback");
        assert_eq!(
            advanced.to_text().unwrap(),
            merged,
            "fallback must still yield a per-node state round-tripping the merged text"
        );
    }

    #[test]
    fn multinode_merge_bootstraps_without_base() {
        // No prior per-node state (first cycle) still merges and yields state.
        let ours = doc_with_exchange_queue("Q.\n\n### Re: q\n\nBody.", "- do [#a1]");
        let theirs = doc_with_exchange_queue("Q.", "- do [#a1]\n- do [#b2]");
        let (merged, state) = MultiNodeState::merge(None, &ours, &theirs).unwrap();
        assert!(merged.contains("Body."));
        assert!(merged.contains("[#b2]"));
        assert_eq!(state.to_text().unwrap(), merged);
    }
}
