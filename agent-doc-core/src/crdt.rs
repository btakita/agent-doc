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
    merge_inner(base_state, ours_text, theirs_text, None)
}

/// Op-capture / evented-reflection merge (`#qnodemerge4`).
///
/// Same contract as [`merge`], but when `theirs_editor_ops` carries the
/// editor's *real* operations (captured from `DocumentListener.documentChanged`
/// / `onDidChangeTextDocument` as insert@offset / delete-range), the `theirs`
/// side of the merge is built by replaying those exact ops onto the CRDT
/// instead of reconstructing them from a Myers text diff.
///
/// A Myers diff picks *a* minimal edit script, not necessarily the one the user
/// actually performed — so two same-region edits can be mis-attributed and
/// duplicate. The captured ops are exact and intention-preserving.
///
/// **Safety gate (the acceptance invariant "ops replay equals editor-observed
/// state"):** the captured ops are used *only* if replaying them onto the
/// resolved merge base reproduces `theirs_text` byte-for-byte. If the ops were
/// captured against a different base (stale / advanced base, missed event),
/// replay will not match and the merge transparently falls back to the
/// diff-guess — never worse than today.
pub fn merge_with_editor_ops(
    base_state: Option<&[u8]>,
    ours_text: &str,
    theirs_text: &str,
    theirs_editor_ops: Option<&[EditorOp]>,
) -> Result<String> {
    merge_inner(base_state, ours_text, theirs_text, theirs_editor_ops)
}

fn merge_inner(
    base_state: Option<&[u8]>,
    ours_text: &str,
    theirs_text: &str,
    theirs_editor_ops: Option<&[EditorOp]>,
) -> Result<String> {
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

    // `#qcellmerge1` op-capture rung (default ON; kill-switch via
    // `AGENT_DOC_CELL_MERGE=0`). When the cell-merge master gate is enabled (the
    // production default) AND the live (theirs) side carries real captured editor
    // ops, route each op to the cell whose span contains it and replay it there,
    // then run the per-cell 3-way join. Ops are absolute offsets against the
    // merge base, so this happens BEFORE any stale-base / prefix-advancement
    // mutation of `base_text`. The op-routed join delegates to `merge_3way`, so
    // it inherits this branch's disjoint-edit `op_level_merge` and
    // conflict-surfacing. On any boundary-crossing / framing op (or stale ops)
    // the cell path signals `fell_back` and we drop through to the existing merge
    // unchanged — where the text-diff `op_level_merge` serves as the degraded
    // fallback. Master-gate explicitly OFF (`=0`) or ops-absent is byte-identical
    // to the legacy path.
    if let Some(ops) = theirs_editor_ops
        && !ops.is_empty()
        && crate::cell_doc::cell_merge_enabled()
    {
        let outcome = crate::cell_doc::merge_3way_with_ops(&base_text, ours_text, theirs_text, ops);
        if !outcome.fell_back {
            if !outcome.conflicts.is_empty() {
                eprintln!(
                    "[crdt] cell_merge(op-routed): {} conflict(s) surfaced (policy=ours-wins)",
                    outcome.conflicts.len()
                );
            }
            eprintln!(
                "[crdt] cell_merge(op-routed): {} captured op(s) routed to cells and replayed",
                ops.len()
            );
            return Ok(outcome.merged_text);
        }
        eprintln!(
            "[crdt] cell_merge(op-routed): fell back to legacy merge_inner path (boundary-crossing/framing/stale ops)"
        );
    }

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

    // Op-capture gate (#qnodemerge4): prefer the editor's real ops for `theirs`
    // when replaying them onto the resolved base reproduces `theirs_text`
    // exactly. Otherwise (no ops, or ops captured against a divergent base) fall
    // back to the diff-guess. Validated against the *resolved* `base_text` so a
    // stale/advanced base correctly disqualifies the captured ops.
    let theirs_replay_ops = match theirs_editor_ops {
        // Conservative offset-safety guard: feed real ops to Yrs only when the
        // base and theirs are ASCII. The op byte-offsets match `apply_ops`'s
        // byte-cursor convention, but Yrs index/offset semantics for non-ASCII
        // text are not asserted here, so unicode regions fall back to the
        // diff-guess (no regression). Per-component merge (#qnodemerge3) means
        // an emoji in `queue` never disables op-replay for ASCII `exchange`
        // prose. (#qnodemerge4)
        Some(ops) if !ops.is_empty() && base_text.is_ascii() && theirs_text.is_ascii() => {
            match replay_editor_ops(&base_text, ops) {
                Some(replayed) if replayed == theirs_text => {
                    eprintln!(
                        "[crdt] editor_ops_replayed: {} captured op(s) reproduce theirs exactly (#qnodemerge4)",
                        ops.len()
                    );
                    Some(ops)
                }
                _ => {
                    eprintln!(
                        "[crdt] editor_ops_fallback: {} captured op(s) do not replay to theirs from the resolved base (stale/misaligned) — using diff-guess (#qnodemerge4)",
                        ops.len()
                    );
                    None
                }
            }
        }
        _ => None,
    };

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

    // Apply theirs edits — from the editor's real ops when the gate passed,
    // otherwise from the diff-guess (#qnodemerge4).
    {
        let text = theirs_doc.get_or_insert_text(TEXT_KEY);
        let mut txn = theirs_doc.transact_mut();
        match theirs_replay_ops {
            Some(ops) => apply_editor_ops(&text, &mut txn, ops),
            None => apply_ops(&text, &mut txn, &theirs_ops),
        }
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

/// Shared `#qcellmerge1` cell-merge seam used by both the text path
/// ([`merge_by_component`]) and the persistence path
/// ([`MultiNodeState::merge`]) so the two entry points compute byte-identical
/// merged text for the same inputs under the flag.
///
/// Returns `Some(merged_text)` when the per-cell 3-way merge succeeds (the
/// caller should return that text and, on the persistence path, rebuild its
/// advanced state from the SAME text). Returns `None` on `fell_back`, so the
/// caller runs its existing legacy merge path unchanged. The caller is
/// responsible for gating this behind [`cell_doc::cell_merge_enabled`].
fn try_cell_merge_text(
    base_text: &str,
    ours_text: &str,
    theirs_text: &str,
    site: &str,
) -> Option<String> {
    let outcome = crate::cell_doc::merge_3way(base_text, ours_text, theirs_text);
    if outcome.fell_back {
        eprintln!("[crdt] cell_merge: fell back to legacy {site} path");
        return None;
    }
    if !outcome.conflicts.is_empty() {
        eprintln!(
            "[crdt] cell_merge: {} conflict(s) surfaced (policy=ours-wins)",
            outcome.conflicts.len()
        );
    }
    Some(outcome.merged_text)
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

    // `#qcellmerge1` cutover seam (default ON, env kill-switch). Unless
    // `AGENT_DOC_CELL_MERGE` is explicitly falsy, attempt the lazily-`reconcile`-
    // based per-cell 3-way merge first. It signals `fell_back` for any structural
    // divergence; on fallback (or the explicit kill-switch) the existing
    // whole-doc / per-node path below runs unchanged.
    if crate::cell_doc::cell_merge_enabled() {
        let base_text = match base_state {
            Some(bytes) => CrdtDoc::decode_state(bytes)
                .map(|d| d.to_text())
                .unwrap_or_default(),
            None => String::new(),
        };
        if let Some(merged) =
            try_cell_merge_text(&base_text, ours_text, theirs_text, "merge_by_component")
        {
            return Ok(merged);
        }
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
    let mut base_by_name: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
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
    let merged: String = merged_nodes.into_iter().map(|(_, text)| text).collect();

    // Structural safety net (#hap7): a per-node leaf merge can, for some shapes
    // (e.g. a compacted-exchange body whose prose mentions the component markers,
    // or an interstitial carrying a stray heading), reassemble into a document
    // whose top-level component framing no longer round-trips — a duplicated or
    // dropped `<!-- /agent:NAME -->` close, or a changed component-name sequence.
    // That malformed shape would then trip the downstream exchange-tail / boundary
    // repair. Rather than emit it, fall back to the proven whole-doc `merge`
    // (logged, never silent) — the same "never worse than today" contract the
    // op-capture gate and the structural-divergence branch above already honor.
    let merged_names: Vec<String> = match segment_into_nodes(&merged) {
        Ok(nodes) => nodes
            .iter()
            .filter_map(|n| n.component_name().map(str::to_string))
            .collect(),
        Err(e) => {
            eprintln!(
                "[crdt] merge_by_component: merged result failed to re-segment ({e}); falling back to whole-doc merge"
            );
            return merge(base_state, ours_text, theirs_text);
        }
    };
    let names_match = merged_names.len() == ours_names.len()
        && merged_names
            .iter()
            .zip(ours_names.iter())
            .all(|(m, o)| m.as_str() == *o);
    if !names_match {
        eprintln!(
            "[crdt] merge_by_component: merged component framing diverged (ours {ours_names:?} != merged {merged_names:?}); falling back to whole-doc merge"
        );
        return merge(base_state, ours_text, theirs_text);
    }

    Ok(merged)
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
        } else if let Some(component_name) = name
            && let Some(merged) = reconcile_component(
                component_name,
                node_base.as_deref(),
                ours_slice,
                theirs_slice,
            )
        {
            // Phase 3 (#qnodemerge3): recursive keyed reconciliation drilled
            // *inside* the component, so two edits to different child nodes (queue
            // items, `### Re:` blocks) reconcile in separate sub-trees and never
            // contend. Falls back to the flat whole-component `merge` below when
            // the component has no keyed child structure or keys are ambiguous.
            merged
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

// ---------------------------------------------------------------------------
// Phase 3 (#qnodemerge3): recursive AST-node keyed reconciliation.
//
// The Phase 1/2 layers reconcile a document into top-level nodes (components +
// interstitials) and run the leaf text `merge` on each whole component. Phase 3
// drills the *same* keyed reconciliation one level deeper, *inside* a component
// whose body is a sequence of keyed children:
//   - `queue`/`backlog`/`review`/`done`: each `- …` list item is a child, keyed
//     by its durable `#id` (the strike paths' identity) or normalized text.
//   - `exchange`: each `### Re:` response block is a child, keyed by its heading.
// Children are matched by key (React-VDOM style): a key present on both sides
// reconciles against its own base child via the leaf `merge`; a key on only one
// side is a pure insert (kept) or delete (honored only on a clean
// delete-vs-unchanged, never for committed `exchange` blocks). Two edits that
// touch different keyed children therefore can never contend, and an in-progress
// edit to one queue item cannot stall or drift another.
// ---------------------------------------------------------------------------

/// Reserved key for the leading text inside a component body before its first
/// keyed child (e.g. the blank line / preamble before the first list item or
/// `### Re:` block). Matched positionally-by-key like any other child.
pub(crate) const PREAMBLE_KEY: &str = "\u{0}::preamble";

/// True for component kinds whose body is a markdown list of keyed items
/// (`queue`/`backlog`/`review`/`done`), reconciled per item in Phase 3.
pub(crate) fn is_list_component(name: &str) -> bool {
    matches!(name, "queue" | "backlog" | "review" | "done")
}

/// One keyed child within a component body. `text` is the exact source slice so
/// that `children.iter().map(|c| &c.text).collect::<String>() == body` (lossless
/// segmentation); `key` is the child's durable identity within the component.
pub(crate) struct KeyedChild {
    pub(crate) key: String,
    pub(crate) text: String,
}

/// First `[#id]` token in `s` (e.g. `do [#qnodemerge3]` → `qnodemerge3`), the
/// durable identity the queue/backlog strike paths key off. `None` for a
/// free-text child with no id.
fn first_hash_id(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        // `[` and `#` are ASCII, so byte indexing stays on char boundaries.
        if bytes[i] == b'[' && bytes[i + 1] == b'#' {
            let rest = &s[i + 2..];
            if let Some(close) = rest.find(']') {
                let id = &rest[..close];
                if !id.is_empty()
                    && id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    return Some(id.to_string());
                }
            }
        }
        i += 1;
    }
    None
}

/// Normalize a list item's identity line for free-text keying: drop the list
/// marker, checkbox, strike markers, and pin glyphs, and collapse whitespace, so
/// a struck/edited item still keys to the same child.
fn normalize_item_text(item: &str) -> String {
    let first = item.lines().next().unwrap_or("").trim();
    let mut s = first.strip_prefix("- ").unwrap_or(first).trim_start();
    for cb in ["[ ]", "[x]", "[X]", "[/]"] {
        if let Some(r) = s.strip_prefix(cb) {
            s = r.trim_start();
            break;
        }
    }
    s.replace("~~", "")
        .replace(":pushpin:", "")
        .replace('📌', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Durable key for a list item: its `#id` when present (stable across strike /
/// text edits), else its normalized text.
fn list_item_key(item: &str) -> String {
    match first_hash_id(item) {
        Some(id) => format!("id:{id}"),
        None => format!("txt:{}", normalize_item_text(item)),
    }
}

/// Durable key for an `### Re:` exchange block: its heading line minus the
/// working-tree-only ` (HEAD)` annotation.
fn exchange_heading_key(block: &str) -> String {
    let heading = block.lines().next().unwrap_or("").trim();
    heading
        .strip_suffix(" (HEAD)")
        .unwrap_or(heading)
        .trim_end()
        .to_string()
}

/// Split a markdown-list component body into keyed children, one per `- …` item
/// (continuation lines attach to the preceding item; leading text becomes a
/// [`PREAMBLE_KEY`] child). Returns `None` when the body has no list item, so
/// the caller falls back to the flat whole-component merge.
pub(crate) fn split_list_children(body: &str) -> Option<Vec<KeyedChild>> {
    let mut starts = Vec::new();
    let mut pos = 0usize;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("- ") || trimmed.trim_end() == "-" {
            starts.push(pos);
        }
        pos += line.len();
    }
    if starts.is_empty() {
        return None;
    }
    Some(segment_by_starts(body, &starts, list_item_key))
}

/// Split an `exchange` component body into keyed children, one per `### Re:`
/// block (leading text becomes a [`PREAMBLE_KEY`] child). `None` when there is
/// no `### Re:` heading.
pub(crate) fn split_exchange_children(body: &str) -> Option<Vec<KeyedChild>> {
    let mut starts = Vec::new();
    let mut pos = 0usize;
    for line in body.split_inclusive('\n') {
        if line.trim_start().starts_with("### Re:") {
            starts.push(pos);
        }
        pos += line.len();
    }
    if starts.is_empty() {
        return None;
    }
    Some(segment_by_starts(body, &starts, exchange_heading_key))
}

/// Build lossless keyed children from child-start byte offsets: a preamble child
/// for any leading text, then one child per `[start, next_start)` slice keyed by
/// `key_of`.
fn segment_by_starts(body: &str, starts: &[usize], key_of: fn(&str) -> String) -> Vec<KeyedChild> {
    let mut children = Vec::with_capacity(starts.len() + 1);
    if starts[0] > 0 {
        children.push(KeyedChild {
            key: PREAMBLE_KEY.to_string(),
            text: body[..starts[0]].to_string(),
        });
    }
    for (i, &s) in starts.iter().enumerate() {
        let e = starts.get(i + 1).copied().unwrap_or(body.len());
        let slice = &body[s..e];
        children.push(KeyedChild {
            key: key_of(slice),
            text: slice.to_string(),
        });
    }
    children
}

/// True when every child key is unique. Keyed reconciliation is only sound with
/// unique keys; a duplicate (e.g. two identical free-text queue items) makes the
/// caller fall back to the flat whole-component merge.
fn keys_unique(children: &[KeyedChild]) -> bool {
    let mut seen = std::collections::HashSet::new();
    children.iter().all(|c| seen.insert(c.key.as_str()))
}

/// Frame a single-component node `text` into `(open_marker, body, close_marker)`
/// where `body` is the content between the markers. `None` when the text does
/// not parse as a component (caller falls back to flat merge).
fn component_framing(text: &str) -> Option<(&str, &str, &str)> {
    let comps = crate::component::parse(text).ok()?;
    let comp = comps.iter().min_by_key(|c| c.open_start)?;
    Some((
        &text[..comp.open_end],
        &text[comp.open_end..comp.close_start],
        &text[comp.close_start..],
    ))
}

/// Recursive keyed reconciliation of one component (`#qnodemerge3`). Splits the
/// component body into keyed children, three-way reconciles them by key, and
/// reassembles the body inside ours' marker framing. Returns `None` to signal
/// "fall back to the flat whole-component `merge`" — for an unsplittable
/// component, malformed framing, or ambiguous (duplicate) keys.
fn reconcile_component(
    name: &str,
    base_text: Option<&str>,
    ours_text: &str,
    theirs_text: &str,
) -> Option<String> {
    if name != "exchange" && !is_list_component(name) {
        return None;
    }
    let (ours_open, ours_body, ours_close) = component_framing(ours_text)?;
    let (_, theirs_body, _) = component_framing(theirs_text)?;
    let base_body = base_text.and_then(|t| component_framing(t).map(|(_, b, _)| b));

    let merged_body = reconcile_component_body(name, base_body, ours_body, theirs_body)?;
    Some(format!("{ours_open}{merged_body}{ours_close}"))
}

/// Three-way reconcile a component body's keyed children. `None` falls back to
/// the flat whole-component merge.
fn reconcile_component_body(
    name: &str,
    base_body: Option<&str>,
    ours_body: &str,
    theirs_body: &str,
) -> Option<String> {
    let split = |b: &str| -> Option<Vec<KeyedChild>> {
        if name == "exchange" {
            split_exchange_children(b)
        } else {
            split_list_children(b)
        }
    };
    let ours_children = split(ours_body)?;
    let theirs_children = split(theirs_body)?;
    let base_children = base_body.and_then(split).unwrap_or_default();

    if !keys_unique(&ours_children)
        || !keys_unique(&theirs_children)
        || !keys_unique(&base_children)
    {
        return None;
    }

    let ours_map: std::collections::HashMap<&str, &KeyedChild> =
        ours_children.iter().map(|c| (c.key.as_str(), c)).collect();
    let theirs_map: std::collections::HashMap<&str, &KeyedChild> = theirs_children
        .iter()
        .map(|c| (c.key.as_str(), c))
        .collect();
    let base_map: std::collections::HashMap<&str, &KeyedChild> =
        base_children.iter().map(|c| (c.key.as_str(), c)).collect();

    let ours_keys: Vec<&str> = ours_children.iter().map(|c| c.key.as_str()).collect();
    let theirs_keys: Vec<&str> = theirs_children.iter().map(|c| c.key.as_str()).collect();
    // Spine selection (#hap7/#qauthorder): for `exchange`, `ours` (the agent side)
    // carries the append-only committed-response history, so it stays the order
    // spine. For the operator-ordered list components (queue/backlog/review/done),
    // `theirs` — the live editor / disk side — carries the operator-authored order,
    // which is authoritative; using it as the spine lets an operator reorder (e.g.
    // moving an interspersed priority prompt to the queue tail) survive the merge
    // instead of being reverted to the snapshot order. Agent-only keys weave in
    // after their nearest preceding placed key either way.
    let order = if name == "exchange" {
        order_union(&ours_keys, &theirs_keys)
    } else {
        order_union(&theirs_keys, &ours_keys)
    };

    // `exchange` is append-only committed history: a `### Re:` block present in
    // the base must never be dropped by a stale/divergent side (the
    // #ipc-crdt-response-drift guard, applied here per block).
    let protect_deletes = name == "exchange";

    // `#queuestatemachine2`/`#qheadresidue`: the operator-ordered list components
    // carry per-item strike lifecycle. Route each matched item through the per-item
    // lifecycle JOIN so a stale persisted-CRDT side cannot un-strike (resurrect) a
    // head that another side already retired. `exchange` blocks are not list items
    // and keep the leaf merge.
    let lifecycle_governed = is_list_component(name);

    let mut out = String::new();
    for key in &order {
        let in_o = ours_map.get(key.as_str());
        let in_t = theirs_map.get(key.as_str());
        let in_b = base_map.get(key.as_str());
        let resolved: Option<String> = match (in_o, in_t) {
            (Some(o), Some(t)) => {
                if o.text == t.text {
                    Some(o.text.clone())
                } else if lifecycle_governed {
                    // Drive the matched item to its lawful state via the per-item
                    // lifecycle lattice (`Live < Struck`), exactly as the
                    // orchestration `transition_queue_item` join specifies. The
                    // highest-ranked side's rendered text governs, so a stale
                    // un-strike from the persisted `.yrs` base/agent side can never
                    // resurrect a head another side already struck.
                    Some(reconcile_list_item_lifecycle(o, t, in_b.copied()))
                } else {
                    // Matched-but-different child: leaf text merge against its
                    // own base child. A leaf merge error falls the whole
                    // component back to the flat merge (returns None).
                    let base_state = in_b.map(|b| CrdtDoc::from_text(&b.text).encode_state());
                    Some(merge(base_state.as_deref(), &o.text, &t.text).ok()?)
                }
            }
            // Present only in ours: insert by ours, or delete by theirs.
            (Some(o), None) => match in_b {
                None => Some(o.text.clone()),
                Some(b) if protect_deletes || o.text != b.text => Some(o.text.clone()),
                Some(_) => None,
            },
            // Present only in theirs: insert by theirs, or delete by ours.
            (None, Some(t)) => match in_b {
                None => Some(t.text.clone()),
                Some(b) if protect_deletes || t.text != b.text => Some(t.text.clone()),
                Some(_) => None,
            },
            (None, None) => None,
        };
        if let Some(text) = resolved {
            out.push_str(&text);
        }
    }
    Some(out)
}

/// Reconcile a matched-but-different list item through the per-item lifecycle
/// lattice (`#queuestatemachine2`/`#qheadresidue`).
///
/// `ours`/`theirs` are the two live sides; `base` is the merge base child (the
/// persisted-CRDT view), if any. Their *visible* lifecycle states
/// ([`QueueItemLifecycle`]) are joined (`Live < Struck`): the merged item takes the
/// rendered text of the side whose lifecycle equals the join, so a stale un-strike
/// can never resurrect a head another side already struck.
///
/// Tie-break within the same lifecycle rank (e.g. both `Live` but differing text —
/// an operator finishing a free-text line while the agent appends) preserves the
/// historical behavior by falling back to the Yrs leaf merge against the base, so
/// genuine concurrent edits at the same lifecycle level still reconcile losslessly.
fn reconcile_list_item_lifecycle(
    ours: &KeyedChild,
    theirs: &KeyedChild,
    base: Option<&KeyedChild>,
) -> String {
    use crate::queue_item_lifecycle::QueueItemLifecycle;

    // The preamble child (leading text before the first list item) carries no
    // item lifecycle — reconcile it as a plain leaf merge.
    if ours.key == PREAMBLE_KEY {
        let base_state = base.map(|b| CrdtDoc::from_text(&b.text).encode_state());
        return merge(base_state.as_deref(), &ours.text, &theirs.text)
            .unwrap_or_else(|_| ours.text.clone());
    }

    let ours_state = QueueItemLifecycle::classify(&ours.text);
    let theirs_state = QueueItemLifecycle::classify(&theirs.text);
    let joined = ours_state.join(theirs_state);

    match (ours_state == joined, theirs_state == joined) {
        // Only one side is at the lawful (joined) lifecycle — that side's rendered
        // text governs. This is the anti-resurrection rung: a struck side beats a
        // stale live side regardless of which input carried it.
        (true, false) => ours.text.clone(),
        (false, true) => theirs.text.clone(),
        // Both sides share the lawful lifecycle level (both Live or both Struck)
        // but differ in text — a genuine concurrent edit at the same level. Fall
        // back to the leaf merge against the base so the edit reconciles losslessly.
        _ => {
            let base_state = base.map(|b| CrdtDoc::from_text(&b.text).encode_state());
            merge(base_state.as_deref(), &ours.text, &theirs.text)
                .unwrap_or_else(|_| ours.text.clone())
        }
    }
}

/// Merge two ordered key sequences into a supersequence: ours' order is the
/// spine, and each theirs-only key is woven in immediately after its nearest
/// preceding theirs key that is already placed (or at the front). Deterministic
/// and order-preserving for the common append/insert-on-one-side cases.
fn order_union(ours_keys: &[&str], theirs_keys: &[&str]) -> Vec<String> {
    let ours_set: std::collections::HashSet<&str> = ours_keys.iter().copied().collect();
    let mut result: Vec<String> = ours_keys.iter().map(|s| s.to_string()).collect();
    for (i, &k) in theirs_keys.iter().enumerate() {
        if ours_set.contains(k) || result.iter().any(|r| r == k) {
            continue;
        }
        let mut insert_pos = 0usize;
        for j in (0..i).rev() {
            if let Some(p) = result.iter().position(|r| r == theirs_keys[j]) {
                insert_pos = p + 1;
                break;
            }
        }
        result.insert(insert_pos, k.to_string());
    }
    result
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
    fn base_lookup(&self) -> Result<(std::collections::HashMap<String, String>, Vec<String>)> {
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
                eprintln!(
                    "[crdt] MultiNodeState::merge: {reason}; falling back to whole-doc merge"
                );
            }
            let merged = merge(base_state.as_deref(), ours_text, theirs_text)?;
            let state = MultiNodeState::from_text(&merged)?;
            Ok((merged, state))
        };

        if ours_text == theirs_text {
            return Ok((ours_text.to_string(), MultiNodeState::from_text(ours_text)?));
        }

        // `#qcellmerge1` persistence-path seam (default ON, env kill-switch).
        // Unless the flag is explicitly falsy, route through the SAME per-cell
        // merge as the text path ([`merge_by_component`]) and rebuild the
        // persisted per-node state from the cell-merged text via
        // [`MultiNodeState::from_text`]. This keeps the returned text and the
        // persisted base round-trip consistent (the persisted base reflects the
        // cell-merge winner, not the legacy per-node winner). On `fell_back` (or
        // the explicit kill-switch) the existing per-node path below runs
        // byte-identically to legacy behavior.
        if crate::cell_doc::cell_merge_enabled() {
            let base_text = match base {
                Some(b) => b.to_text().unwrap_or_default(),
                None => String::new(),
            };
            if let Some(merged) =
                try_cell_merge_text(&base_text, ours_text, theirs_text, "MultiNodeState::merge")
            {
                let state = MultiNodeState::from_text(&merged)?;
                return Ok((merged, state));
            }
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

        let merged_nodes =
            merge_aligned_nodes(&ours_nodes, &theirs_nodes, |name, idx| match name {
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

/// A real editor operation captured from the editor's change event
/// (`DocumentListener.documentChanged` / `onDidChangeTextDocument`),
/// expressed as an absolute-offset mutation in **byte** units — consistent
/// with the byte-offset cursor convention used throughout this module's Yrs
/// apply path ([`apply_ops`]).
///
/// A replacement (the editor reports an old fragment + a new fragment at the
/// same offset) is captured as a [`EditorOp::Delete`] of the old length
/// followed by an [`EditorOp::Insert`] of the new text at the same offset; the
/// reporter is responsible for that split so replay stays a flat, ordered
/// sequence. Ops are recorded and replayed in the order the editor performed
/// them, so each op's offset is absolute against the document state *after* all
/// prior ops in the sequence (exactly how the editor's own events behave).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum EditorOp {
    /// Insert `text` at byte `offset`.
    Insert { offset: usize, text: String },
    /// Delete `len` bytes starting at byte `offset`.
    Delete { offset: usize, len: usize },
}

/// Replay captured editor ops onto `base`, reconstructing the editor's final
/// text. Ops apply in sequence with each offset absolute against the running
/// buffer (matching the editor's own event semantics).
///
/// Returns `None` if any op is out of bounds or would land on a non-UTF-8
/// char boundary — the caller treats `None` as "these ops are stale or
/// misaligned, fall back to the text diff" (the `#qnodemerge4` safety gate).
/// Returning the reconstructed text lets the merge assert it equals the
/// editor-observed `theirs` before trusting the ops.
pub fn replay_editor_ops(base: &str, ops: &[EditorOp]) -> Option<String> {
    let mut buf = base.to_string();
    for op in ops {
        match op {
            EditorOp::Insert { offset, text } => {
                if *offset > buf.len() || !buf.is_char_boundary(*offset) {
                    return None;
                }
                buf.insert_str(*offset, text);
            }
            EditorOp::Delete { offset, len } => {
                let end = offset.checked_add(*len)?;
                if end > buf.len() || !buf.is_char_boundary(*offset) || !buf.is_char_boundary(end) {
                    return None;
                }
                buf.replace_range(*offset..end, "");
            }
        }
    }
    Some(buf)
}

/// Apply captured editor ops directly to a Yrs text type, in order, as
/// absolute-offset mutations (insert@offset / remove len@offset). This feeds
/// the editor's *real* operation sequence into the CRDT so a concurrent agent
/// edit merges against the user's actual edit boundaries rather than against a
/// Myers-diff reconstruction (`#qnodemerge4`). Offsets are byte offsets,
/// consistent with [`apply_ops`]. Callers must validate the ops with
/// [`replay_editor_ops`] first; this assumes in-bounds, char-aligned offsets.
fn apply_editor_ops(text: &TextRef, txn: &mut yrs::TransactionMut<'_>, ops: &[EditorOp]) {
    for op in ops {
        match op {
            EditorOp::Insert { offset, text: s } => {
                text.insert(txn, *offset as u32, s);
            }
            EditorOp::Delete { offset, len } => {
                text.remove_range(txn, *offset as u32, *len as u32);
            }
        }
    }
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

    // ---- #qnodemerge4: op-capture / evented reflection ----

    #[test]
    fn replay_editor_ops_reconstructs_insert() {
        let base = "hello world\n";
        // user typed "!" after "hello"
        let ops = vec![EditorOp::Insert {
            offset: 5,
            text: "!".to_string(),
        }];
        assert_eq!(
            replay_editor_ops(base, &ops).as_deref(),
            Some("hello! world\n")
        );
    }

    #[test]
    fn replay_editor_ops_reconstructs_delete_and_replace_sequence() {
        let base = "- do [#foo]\n";
        // user renamed #foo -> #foobar: delete "foo" then insert "foobar"
        let ops = vec![
            EditorOp::Delete { offset: 7, len: 3 },
            EditorOp::Insert {
                offset: 7,
                text: "foobar".to_string(),
            },
        ];
        assert_eq!(
            replay_editor_ops(base, &ops).as_deref(),
            Some("- do [#foobar]\n")
        );
    }

    #[test]
    fn replay_editor_ops_sequential_offsets_are_post_prior_op() {
        // Each op's offset is absolute against the running buffer, like the
        // editor's own events: insert "X" at 0, then "Y" at 2 (after "Xa").
        let base = "ab";
        let ops = vec![
            EditorOp::Insert {
                offset: 0,
                text: "X".to_string(),
            },
            EditorOp::Insert {
                offset: 2,
                text: "Y".to_string(),
            },
        ];
        assert_eq!(replay_editor_ops(base, &ops).as_deref(), Some("XaYb"));
    }

    #[test]
    fn replay_editor_ops_out_of_bounds_is_none() {
        let base = "short";
        let insert = vec![EditorOp::Insert {
            offset: 99,
            text: "x".to_string(),
        }];
        assert_eq!(replay_editor_ops(base, &insert), None);
        let delete = vec![EditorOp::Delete { offset: 3, len: 99 }];
        assert_eq!(replay_editor_ops(base, &delete), None);
    }

    #[test]
    fn replay_editor_ops_non_char_boundary_is_none() {
        // "é" is two bytes (0xC3 0xA9); offset 1 splits it.
        let base = "é";
        let ops = vec![EditorOp::Insert {
            offset: 1,
            text: "x".to_string(),
        }];
        assert_eq!(replay_editor_ops(base, &ops), None);
    }

    #[test]
    fn editor_op_json_roundtrip() {
        let ops = vec![
            EditorOp::Insert {
                offset: 3,
                text: "hi".to_string(),
            },
            EditorOp::Delete { offset: 0, len: 2 },
        ];
        let json = serde_json::to_string(&ops).unwrap();
        let back: Vec<EditorOp> = serde_json::from_str(&json).unwrap();
        assert_eq!(ops, back);
    }

    #[test]
    fn merge_with_editor_ops_gate_passes_and_preserves_user_edit() {
        // base: a queue item the user renames while the agent appends a response
        // to a different region. Editor op for theirs is exact.
        let base = "<!-- agent:queue -->\n- do [#foo]\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- do [#foo]\n<!-- /agent:queue -->\nAGENT RESPONSE\n";
        let theirs = "<!-- agent:queue -->\n- do [#foobar]\n<!-- /agent:queue -->\n";
        let ops = vec![
            EditorOp::Delete { offset: 28, len: 3 },
            EditorOp::Insert {
                offset: 28,
                text: "foobar".to_string(),
            },
        ];
        // Verify the ops actually reconstruct theirs (the gate input).
        assert_eq!(replay_editor_ops(base, &ops).as_deref(), Some(theirs));

        let base_state = CrdtDoc::from_text(base).encode_state();
        let merged = merge_with_editor_ops(Some(&base_state), ours, theirs, Some(&ops)).unwrap();
        // Both edits present, renamed item, no duplication.
        assert!(merged.contains("- do [#foobar]"), "merged: {merged}");
        assert!(merged.contains("AGENT RESPONSE"), "merged: {merged}");
        assert!(
            !merged.contains("- do [#foo]\n"),
            "stale item leaked: {merged}"
        );
        assert_eq!(
            merged.matches("do [#foobar]").count(),
            1,
            "duplicated: {merged}"
        );
    }

    #[test]
    fn merge_with_editor_ops_stale_ops_fall_back_to_diff() {
        // Ops captured against a DIFFERENT base than the resolved merge base:
        // replay won't equal theirs, so the merge must fall back to the diff
        // path and still produce the same result as plain `merge`.
        let base = "Line A\nLine B\n";
        let ours = "Line A\nLine B\nAgent line\n";
        let theirs = "Line A\nLine B edited\n";
        // Bogus ops that do NOT reconstruct theirs from base.
        let bogus = vec![EditorOp::Insert {
            offset: 0,
            text: "ZZZ".to_string(),
        }];
        assert_ne!(replay_editor_ops(base, &bogus).as_deref(), Some(theirs));

        let base_state = CrdtDoc::from_text(base).encode_state();
        let with_ops =
            merge_with_editor_ops(Some(&base_state), ours, theirs, Some(&bogus)).unwrap();
        let diff_only = merge(Some(&base_state), ours, theirs).unwrap();
        assert_eq!(
            with_ops, diff_only,
            "stale ops must fall back to the diff-guess result"
        );
        // The bogus "ZZZ" must NOT have leaked into the merged output.
        assert!(!with_ops.contains("ZZZ"), "stale op leaked: {with_ops}");
    }

    #[test]
    fn merge_with_editor_ops_none_matches_plain_merge() {
        // The op-aware entry point with None ops is byte-identical to `merge`.
        let base = "header\n\nbody\n";
        let ours = "header\n\nbody\nagent\n";
        let theirs = "header\n\nbody edited\n";
        let base_state = CrdtDoc::from_text(base).encode_state();
        assert_eq!(
            merge_with_editor_ops(Some(&base_state), ours, theirs, None).unwrap(),
            merge(Some(&base_state), ours, theirs).unwrap()
        );
    }

    #[test]
    fn merge_with_editor_ops_empty_ops_fall_back() {
        let base = "x\ny\n";
        let ours = "x\ny\nagent\n";
        let theirs = "x\ny edited\n";
        let base_state = CrdtDoc::from_text(base).encode_state();
        let empty: Vec<EditorOp> = vec![];
        assert_eq!(
            merge_with_editor_ops(Some(&base_state), ours, theirs, Some(&empty)).unwrap(),
            merge(Some(&base_state), ours, theirs).unwrap()
        );
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
        assert!(
            merged.contains("[#b2]"),
            "operator queue edit lost:\n{merged}"
        );

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

        let ours = doc_with_exchange_queue(
            &format!("Q.\n\n### Re: q\n\n{console}"),
            "- :pushpin: typing here",
        );
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
        assert_eq!(
            by_component, whole_doc,
            "structural divergence must use whole-doc fallback"
        );
    }

    // ---- #qnodemerge2: per-node CRDT state persistence --------------------

    #[test]
    fn multinode_state_roundtrip_encode_decode() {
        // Per-component encode/decode round-trips through the container format and
        // reconstructs the exact document text.
        let doc =
            doc_with_exchange_queue("Prompt.\n\n### Re: q\n\nBody.", "- do [#a1]\n- do [#b2]");
        let state = MultiNodeState::from_text(&doc).unwrap();
        let encoded = state.encode();
        let decoded = MultiNodeState::decode(&encoded).unwrap();
        assert_eq!(
            decoded, state,
            "decode must reproduce the encoded per-node state"
        );
        assert_eq!(
            decoded.to_text().unwrap(),
            doc,
            "per-node state must reconstruct the original document text"
        );
        // The container is self-describing — one node per interstitial+component.
        assert!(
            decoded.nodes.len() >= 4,
            "expected ≥4 nodes, got {}",
            decoded.nodes.len()
        );
        assert!(
            decoded
                .nodes
                .iter()
                .any(|n| n.name.as_deref() == Some("exchange"))
        );
        assert!(
            decoded
                .nodes
                .iter()
                .any(|n| n.name.as_deref() == Some("queue"))
        );
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
            migrated
                .nodes
                .iter()
                .any(|n| n.name.as_deref() == Some("queue")),
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
        assert!(
            merged.contains("Agent body."),
            "exchange edit lost:\n{merged}"
        );

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
        assert!(
            merged.contains("### Re: existing"),
            "exchange edit lost:\n{merged}"
        );
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
        assert_eq!(
            merged, whole_doc,
            "structural divergence must use whole-doc fallback"
        );
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

    // ---- #qcellmerge1 persistence round-trip: MultiNodeState::merge seam ----
    //
    // These prove the live-persistence path (`MultiNodeState::merge`, which
    // returns BOTH the merged text AND the advanced per-node state persisted to
    // `<hash>.nodes.yrs`) routes through the SAME per-cell merge as the text path
    // (`merge_by_component`) under the flag, so the persisted base reflects the
    // cell-merge winner and the round-trip is consistent across cycles.

    /// Scoped guard: set `AGENT_DOC_CELL_MERGE=1` for the duration, restore on
    /// drop. Serialized through the shared crate lock so it can't race the
    /// `cell_doc` flag tests.
    struct CellMergeFlagOn {
        _guard: std::sync::MutexGuard<'static, ()>,
    }
    impl CellMergeFlagOn {
        fn on() -> Self {
            let guard = crate::cell_doc::CELL_MERGE_ENV_LOCK.lock().unwrap();
            // SAFETY: single-threaded under the lock; restored on drop.
            unsafe {
                std::env::set_var(crate::cell_doc::CELL_MERGE_ENV, "1");
            }
            assert!(crate::cell_doc::cell_merge_enabled());
            CellMergeFlagOn { _guard: guard }
        }
    }
    impl Drop for CellMergeFlagOn {
        fn drop(&mut self) {
            // SAFETY: still holding the lock.
            unsafe {
                std::env::remove_var(crate::cell_doc::CELL_MERGE_ENV);
            }
        }
    }

    #[test]
    fn multinode_merge_conflict_persists_cell_winner_under_flag() {
        // A same-item conflict (both sides edit the SAME queue line differently).
        // With the flag ON, MultiNodeState::merge returns text T and state S; the
        // persisted state S must re-encode to a doc whose queue component matches
        // T (the persisted base reflects the cell-merge winner, NOT a legacy
        // per-node winner that could diverge from T). queue is operator-owned, so
        // the winner is THEIRS (#provauth1/#provauth4).
        let _flag = CellMergeFlagOn::on();

        let base = doc_with_exchange_queue("Q.", "- do [#a1] original");
        let base_state = MultiNodeState::from_text(&base).unwrap();
        let ours = doc_with_exchange_queue("Q.", "- do [#a1] OURS edit");
        let theirs = doc_with_exchange_queue("Q.", "- do [#a1] THEIRS edit");

        let (merged, state) = MultiNodeState::merge(Some(&base_state), &ours, &theirs).unwrap();

        // The persisted state must round-trip to the SAME text that was returned.
        assert_eq!(
            state.to_text().unwrap(),
            merged,
            "persisted state diverges from returned merged text under conflict"
        );
        // The returned text reflects the cell-merge winner — theirs, since queue
        // is operator-owned.
        assert!(
            merged.contains("THEIRS edit"),
            "cell-merge operator-owned theirs-wins winner lost:\n{merged}"
        );
        // The persisted queue component must hold the winner, not the legacy text.
        let persisted = state.to_text().unwrap();
        let queue_body = persisted
            .split("<!-- agent:queue -->")
            .nth(1)
            .and_then(|s| s.split("<!-- /agent:queue -->").next())
            .unwrap_or("");
        assert!(
            queue_body.contains("THEIRS edit"),
            "persisted base lost the cell-merge winner:\n{queue_body}"
        );
    }

    #[test]
    fn multinode_merge_struck_head_stable_across_persistence_cycles() {
        // Multi-cycle no-churn under persistence: a struck head must stay struck
        // across cycles when the advanced MultiNodeState is carried forward as the
        // next cycle's base AND a stale un-strike keeps arriving. The persisted
        // state must not oscillate — a no-new-edit cycle is idempotent.
        let _flag = CellMergeFlagOn::on();

        // Cycle 0: strike lands (ours strikes, theirs is the pre-strike base).
        let base0 = doc_with_exchange_queue("Q.", "- do [#z9] answered");
        let base0_state = MultiNodeState::from_text(&base0).unwrap();
        let ours0 = doc_with_exchange_queue("Q.", "- ~~do [#z9] answered~~");
        let theirs0 = doc_with_exchange_queue("Q.", "- do [#z9] answered");
        let (text0, state0) = MultiNodeState::merge(Some(&base0_state), &ours0, &theirs0).unwrap();
        assert!(
            text0.contains("~~do [#z9] answered~~"),
            "strike lost:\n{text0}"
        );
        assert_eq!(
            state0.to_text().unwrap(),
            text0,
            "cycle0 round-trip diverges"
        );

        // Cycle 1: carry state0 forward as base; a STALE un-strike side arrives.
        // The struck head must stay struck (anti-resurrection), and the persisted
        // state must round-trip the returned text.
        let ours1 = doc_with_exchange_queue("Q.", "- do [#z9] answered"); // stale un-strike
        let theirs1 = doc_with_exchange_queue("Q.", "- ~~do [#z9] answered~~");
        let (text1, state1) = MultiNodeState::merge(Some(&state0), &ours1, &theirs1).unwrap();
        assert!(
            text1.contains("~~do [#z9] answered~~"),
            "stale un-strike resurrected the head:\n{text1}"
        );
        let live1 = {
            let body = text1
                .split("<!-- agent:queue -->")
                .nth(1)
                .and_then(|s| s.split("<!-- /agent:queue -->").next())
                .unwrap_or("");
            body.lines()
                .filter(|l| l.contains("do [#z9]") && !l.contains("~~"))
                .count()
        };
        assert_eq!(
            live1, 0,
            "head resurrected LIVE across persistence:\n{text1}"
        );
        assert_eq!(
            state1.to_text().unwrap(),
            text1,
            "cycle1 round-trip diverges"
        );

        // Cycle 2: NO new edits (ours == theirs == carried text). Must be
        // idempotent — identical text AND identical persisted state to cycle 1.
        let (text2, state2) = MultiNodeState::merge(Some(&state1), &text1, &text1).unwrap();
        assert_eq!(text2, text1, "no-edit cycle oscillated the text");
        assert_eq!(
            state2.encode(),
            state1.encode(),
            "no-edit cycle oscillated the persisted state"
        );
    }

    #[test]
    fn multinode_merge_flag_off_byte_identical_to_pre_rung() {
        // Flag-OFF parity: with the kill-switch explicitly set, MultiNodeState::merge
        // output must be byte-identical to the legacy per-node path (the seam is a
        // strict no-op). We compute the legacy result by constructing the same
        // per-node merge the function would run without the seam. Per-cell merge is
        // default-ON, so the legacy path is exercised via the explicit kill-switch.
        let _guard = crate::cell_doc::CELL_MERGE_ENV_LOCK.lock().unwrap();
        // SAFETY: serialized under the lock.
        unsafe {
            std::env::set_var(crate::cell_doc::CELL_MERGE_ENV, "0");
        }
        assert!(!crate::cell_doc::cell_merge_enabled(), "must be OFF");

        let base = doc_with_exchange_queue("Existing prompt.", "- do [#a1]");
        let base_state = MultiNodeState::from_text(&base).unwrap();
        let ours = doc_with_exchange_queue(
            "Existing prompt.\n\n### Re: existing — opus\n\nAgent response body.",
            "- do [#a1]",
        );
        let theirs = doc_with_exchange_queue("Existing prompt.", "- do [#a1]\n- do [#b2]");

        // Two flag-OFF runs are deterministic and identical.
        let (off1, off1_state) = MultiNodeState::merge(Some(&base_state), &ours, &theirs).unwrap();
        let (off2, off2_state) = MultiNodeState::merge(Some(&base_state), &ours, &theirs).unwrap();
        assert_eq!(off1, off2, "flag-OFF text not deterministic");
        assert_eq!(
            off1_state.encode(),
            off2_state.encode(),
            "flag-OFF state not deterministic"
        );
        // The legacy per-node path produced the expected isolated merge.
        assert!(off1.contains("### Re: existing"));
        assert!(off1.contains("[#b2]"));

        // Restore the default (ON) so a leaked kill-switch can't poison sibling
        // tests that read the master gate without holding the lock.
        // SAFETY: still holding the lock.
        unsafe {
            std::env::remove_var(crate::cell_doc::CELL_MERGE_ENV);
        }
    }

    #[test]
    fn cell_seams_produce_identical_text_under_flag() {
        // Round-trip equality: for a non-conflicting concurrent edit, the text
        // from merge_by_component (flag ON) must equal the text from
        // MultiNodeState::merge (flag ON) for the SAME inputs — the two seams
        // agree, so the persisted base never diverges from the committed doc.
        let _flag = CellMergeFlagOn::on();

        let base = doc_with_exchange_queue("Existing prompt.", "- do [#a1]");
        let ours = doc_with_exchange_queue(
            "Existing prompt.\n\n### Re: existing — opus\n\nAgent response body.",
            "- do [#a1]",
        );
        let theirs = doc_with_exchange_queue("Existing prompt.", "- do [#a1]\n- do [#b2]");

        let text_seam = {
            let base_state = CrdtDoc::from_text(&base).encode_state();
            merge_by_component(Some(&base_state), &ours, &theirs).unwrap()
        };
        let (persist_seam, _state) = {
            let base_state = MultiNodeState::from_text(&base).unwrap();
            MultiNodeState::merge(Some(&base_state), &ours, &theirs).unwrap()
        };
        assert_eq!(
            text_seam, persist_seam,
            "the two cell-merge seams disagree on merged text:\n--- text path ---\n{text_seam}\n--- persist path ---\n{persist_seam}"
        );
    }

    // ---- #qnodemerge3: recursive AST-node keyed reconciliation ------------

    #[test]
    fn reconcile_queue_item_edit_does_not_touch_sibling_item() {
        // Acceptance (a): the operator edits queue item B (#b2) while item A
        // (#a1) is the running head and the agent appends an exchange response.
        // Item A must survive byte-for-byte (no drift, no duplication); item B
        // carries the operator edit. Two edits to different keyed children must
        // reconcile in separate sub-trees.
        let base = doc_with_exchange_queue("Q.", "- :pushpin: do [#a1]\n- :pushpin: do [#b2]");
        let base_state = CrdtDoc::from_text(&base).encode_state();
        let ours = doc_with_exchange_queue(
            "Q.\n\n### Re: q — opus\n\nBody.",
            "- :pushpin: do [#a1]\n- :pushpin: do [#b2]",
        );
        let theirs = doc_with_exchange_queue(
            "Q.",
            "- :pushpin: do [#a1]\n- :pushpin: do [#b2] with operator note",
        );

        let merged = merge_by_component(Some(&base_state), &ours, &theirs).unwrap();
        assert!(
            merged.contains("### Re: q"),
            "exchange edit lost:\n{merged}"
        );
        assert_eq!(
            merged.matches("do [#a1]").count(),
            1,
            "sibling item A drifted/duplicated:\n{merged}"
        );
        assert!(
            merged.contains("- :pushpin: do [#a1]\n"),
            "sibling item A text drifted:\n{merged}"
        );
        assert!(
            merged.contains("operator note"),
            "item B operator edit lost:\n{merged}"
        );
    }

    #[test]
    fn reconcile_exchange_prompt_between_blocks_converges_with_new_block() {
        // Acceptance (b): a user prompt inserted between two committed `### Re:`
        // blocks while the agent appends a new block must converge with all three
        // blocks present and zero cross-block splice (each heading exactly once).
        let r1 = "### Re: first — opus\n\nFirst answer.\n\n";
        let r2 = "### Re: second — opus\n\nSecond answer.\n\n";
        let base = doc_with_exchange_queue(&format!("{r1}{r2}"), "- do [#a1]");
        let base_state = CrdtDoc::from_text(&base).encode_state();

        let r3 = "### Re: third — opus\n\nThird answer.\n\n";
        let ours = doc_with_exchange_queue(&format!("{r1}{r2}{r3}"), "- do [#a1]");
        let theirs = doc_with_exchange_queue(
            &format!("{r1}❯ a prompt typed between blocks\n\n{r2}"),
            "- do [#a1]",
        );

        let merged = merge_by_component(Some(&base_state), &ours, &theirs).unwrap();
        assert!(merged.contains("### Re: first"), "block 1 lost:\n{merged}");
        assert!(merged.contains("### Re: second"), "block 2 lost:\n{merged}");
        assert!(
            merged.contains("### Re: third"),
            "appended block 3 lost:\n{merged}"
        );
        assert!(
            merged.contains("a prompt typed between blocks"),
            "interleaved operator prompt lost:\n{merged}"
        );
        assert_eq!(
            merged.matches("### Re: first").count(),
            1,
            "block 1 cross-spliced/duplicated:\n{merged}"
        );
        assert_eq!(
            merged.matches("### Re: third").count(),
            1,
            "block 3 cross-spliced/duplicated:\n{merged}"
        );
    }

    #[test]
    fn reconcile_queue_preserves_concurrent_user_item_addition() {
        // The #queue-user-edit-overwrite class: the operator adds a new queue item
        // (theirs) while the agent edits a different item (ours). Keyed
        // reconciliation treats the new item as a pure insert — never dropped.
        let base = doc_with_exchange_queue("Q.", "- do [#a1]");
        let base_state = CrdtDoc::from_text(&base).encode_state();
        let ours = doc_with_exchange_queue("Q.", "- do [#a1] agent touched");
        let theirs = doc_with_exchange_queue("Q.", "- do [#a1]\n- do [#user-added]");

        let merged = merge_by_component(Some(&base_state), &ours, &theirs).unwrap();
        assert!(
            merged.contains("[#user-added]"),
            "concurrent user queue addition dropped:\n{merged}"
        );
        assert!(
            merged.contains("agent touched"),
            "concurrent agent item edit lost:\n{merged}"
        );
    }

    #[test]
    fn reconcile_queue_honors_clean_item_deletion() {
        // theirs removes item B (clean delete: ours left B == base). The delete is
        // honored while item A and the agent's exchange edit survive.
        let base = doc_with_exchange_queue("Q.", "- do [#a1]\n- do [#b2]");
        let base_state = CrdtDoc::from_text(&base).encode_state();
        let ours = doc_with_exchange_queue("Q.\n\n### Re: q\n\nBody.", "- do [#a1]\n- do [#b2]");
        let theirs = doc_with_exchange_queue("Q.", "- do [#a1]");

        let merged = merge_by_component(Some(&base_state), &ours, &theirs).unwrap();
        assert!(merged.contains("Body."), "exchange edit lost:\n{merged}");
        let queue_body = merged
            .split("<!-- agent:queue -->")
            .nth(1)
            .and_then(|s| s.split("<!-- /agent:queue -->").next())
            .unwrap_or("");
        assert!(queue_body.contains("[#a1]"), "item A lost:\n{queue_body}");
        assert!(
            !queue_body.contains("[#b2]"),
            "clean deletion of item B not honored:\n{queue_body}"
        );
    }

    #[test]
    fn reconcile_exchange_never_drops_committed_block_on_stale_side() {
        // A stale `theirs` that lost a committed `### Re:` block must not delete it
        // — committed exchange history is append-only (the per-block
        // #ipc-crdt-response-drift guard).
        let r1 = "### Re: first — opus\n\nFirst answer.\n\n";
        let r2 = "### Re: second — opus\n\nSecond answer.\n\n";
        let base = doc_with_exchange_queue(&format!("{r1}{r2}"), "- do [#a1]");
        let base_state = CrdtDoc::from_text(&base).encode_state();
        // ours keeps both committed blocks; theirs is stale and dropped block 2.
        let ours = doc_with_exchange_queue(&format!("{r1}{r2}"), "- do [#a1]\n- do [#new]");
        let theirs = doc_with_exchange_queue(&r1.to_string(), "- do [#a1]");

        let merged = merge_by_component(Some(&base_state), &ours, &theirs).unwrap();
        assert!(
            merged.contains("### Re: second"),
            "stale side deleted a committed block:\n{merged}"
        );
        assert!(
            merged.contains("[#new]"),
            "concurrent queue add lost:\n{merged}"
        );
    }

    #[test]
    fn reconcile_falls_back_on_duplicate_item_keys() {
        // Two identical free-text queue items make keys ambiguous → fall back to
        // the flat whole-component merge (no panic, deterministic, content kept).
        let base = doc_with_exchange_queue("Q.", "- repeated item\n- repeated item");
        let base_state = CrdtDoc::from_text(&base).encode_state();
        let ours = doc_with_exchange_queue("Q.", "- repeated item\n- repeated item\n- new one");
        let theirs = doc_with_exchange_queue("Q.", "- repeated item\n- repeated item");

        let merged = merge_by_component(Some(&base_state), &ours, &theirs).unwrap();
        assert!(
            merged.contains("new one"),
            "duplicate-key fallback dropped the new item:\n{merged}"
        );
    }

    #[test]
    fn first_hash_id_extracts_durable_id() {
        assert_eq!(
            first_hash_id("- :pushpin: do [#qnodemerge3]"),
            Some("qnodemerge3".to_string())
        );
        assert_eq!(
            first_hash_id("- [ ] [#6b5hwire] some text"),
            Some("6b5hwire".to_string())
        );
        assert_eq!(first_hash_id("- free text no id"), None);
    }

    #[test]
    fn list_item_key_stable_across_strike_and_glyphs() {
        // A struck or pin-glyphed item keys the same as its plain form, so a
        // strike reconciles as a content edit, not a delete+insert.
        assert_eq!(list_item_key("- do [#a1]"), list_item_key("- ~~do [#a1]~~"));
        assert_eq!(
            list_item_key("- :pushpin: hello world"),
            list_item_key("- ~~hello world~~")
        );
    }

    #[test]
    fn order_union_weaves_theirs_only_inserts() {
        let ours = ["a", "b", "c"];
        let theirs = ["a", "x", "b", "c", "y"];
        assert_eq!(
            order_union(&ours, &theirs),
            vec![
                "a".to_string(),
                "x".into(),
                "b".into(),
                "c".into(),
                "y".into()
            ]
        );
    }

    #[test]
    fn split_list_children_is_lossless() {
        // The segmentation must be exact: concatenating child texts reproduces the
        // body byte-for-byte (no content gained or lost).
        let body = "\n- do [#a1]\n  continuation line\n- :pushpin: do [#b2]\n\n";
        let children = split_list_children(body).unwrap();
        let joined: String = children.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(joined, body, "list segmentation is not lossless");
    }

    #[test]
    fn split_exchange_children_is_lossless() {
        let body = "\n### Re: a — opus\n\nbody a\n\n### Re: b — opus\n\nbody b\n";
        let children = split_exchange_children(body).unwrap();
        let joined: String = children.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(joined, body, "exchange segmentation is not lossless");
    }

    /// #qheadresidue / #queuestatemachine2: a free-text queue head answered and
    /// STRUCK in the committed doc (`ours`) must NOT be resurrected un-struck by a
    /// stale persisted-CRDT side (`theirs`) that still carries the head live.
    ///
    /// This drives the real `merge_by_component` convergence path. The struck and
    /// un-struck lines share the same `list_item_key` (strike markers are
    /// normalized away), so the per-item reconciler leaf-merges them — and the
    /// stale live text resurrects.
    #[test]
    fn merge_by_component_struck_free_text_head_not_resurrected_by_stale_crdt() {
        // Base: the head is a live free-text queue item.
        let base = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n- Still getting JB File Cache Conflict dialogs.\n<!-- /agent:queue -->\n";
        // Ours (committed doc, authoritative): the agent answered and STRUCK it.
        let ours = "<!-- agent:exchange -->\n### Re: topic\nAgent response body.\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n- ~~Still getting JB File Cache Conflict dialogs.~~\n<!-- /agent:queue -->\n";
        // Theirs (persisted CRDT, stale): still carries the head un-struck.
        let theirs = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n- Still getting JB File Cache Conflict dialogs.\n<!-- /agent:queue -->\n";

        let base_state = CrdtDoc::from_text(base).encode_state();
        let merged = merge_by_component(Some(&base_state), ours, theirs).unwrap();

        let queue_body = merged
            .split("<!-- agent:queue -->")
            .nth(1)
            .and_then(|s| s.split("<!-- /agent:queue -->").next())
            .unwrap();
        // The head must remain struck — never reappear as a fresh live head.
        assert!(
            queue_body.contains("~~Still getting JB File Cache Conflict dialogs.~~"),
            "struck head must survive struck:\n{queue_body}"
        );
        let live_occurrences = queue_body
            .lines()
            .filter(|l| {
                l.contains("Still getting JB File Cache Conflict dialogs.") && !l.contains("~~")
            })
            .count();
        assert_eq!(
            live_occurrences, 0,
            "stale CRDT resurrected the struck head as a live queue line:\n{queue_body}"
        );
    }

    /// #qheadresidue variant: the answered head was REAPED (removed) from the
    /// committed doc (`ours`), but the stale persisted CRDT (`theirs`) still
    /// carries it live. The merge must NOT re-add the reaped head.
    ///
    /// This is the canonical "answered head reappears every preflight" residue:
    /// the committed queue is empty, the persisted `.yrs` still has the line, and
    /// the per-item reconciler treats theirs-only as a clean insert.
    #[test]
    fn merge_by_component_reaped_free_text_head_not_readded_by_stale_crdt() {
        let base = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n- Still getting JB File Cache Conflict dialogs.\n<!-- /agent:queue -->\n";
        // Ours (committed): the head is gone — reaped after answering.
        let ours = "<!-- agent:exchange -->\n### Re: topic\nAgent response body.\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n<!-- /agent:queue -->\n";
        // Theirs (stale persisted CRDT): still carries the head live.
        let theirs = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n- Still getting JB File Cache Conflict dialogs.\n<!-- /agent:queue -->\n";

        let base_state = CrdtDoc::from_text(base).encode_state();
        let merged = merge_by_component(Some(&base_state), ours, theirs).unwrap();

        let queue_body = merged
            .split("<!-- agent:queue -->")
            .nth(1)
            .and_then(|s| s.split("<!-- /agent:queue -->").next())
            .unwrap();
        assert!(
            !queue_body.contains("Still getting JB File Cache Conflict dialogs."),
            "stale CRDT re-added the reaped head:\n{queue_body}"
        );
    }

    /// #qheadresidue / #queuestatemachine2 — THE genuinely-red resurrection case.
    ///
    /// This is the exact live churn: a queue head that was already answered+struck
    /// (persisted into the `.yrs` BASE as `~~...~~`) is "un-struck" by the stale
    /// agent side (`ours`), which still carries the head LIVE because the seam
    /// strike was a direct file edit the CRDT base never saw advance. The
    /// committed disk side (`theirs`) is correctly struck. A plain Yrs leaf merge
    /// reads `ours` as having DELETED the `~~` markers (an edit vs base) while
    /// `theirs` matches base, so the un-strike "edit" wins and the head reappears
    /// LIVE every cycle.
    ///
    /// Before the per-item lifecycle wiring this asserts FAILED (merged dropped
    /// the strike → live head). The fix joins each side through the queue-item
    /// lifecycle SM: `Struck` outranks `Authored`/`Mirrored`, so a stale un-strike
    /// can never regress a struck head.
    #[test]
    fn merge_by_component_struck_id_head_not_resurrected_by_stale_unstrike() {
        // BASE: already struck (the answered state persisted into the .yrs).
        let base = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n- ~~do [#abcd] fix the thing~~\n<!-- /agent:queue -->\n";
        // OURS (agent / content_ours side): still LIVE — un-strikes vs base.
        let ours = "<!-- agent:exchange -->\n### Re: topic\nAgent response body.\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n- do [#abcd] fix the thing\n<!-- /agent:queue -->\n";
        // THEIRS (committed disk): correctly struck.
        let theirs = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n- ~~do [#abcd] fix the thing~~\n<!-- /agent:queue -->\n";

        let base_state = CrdtDoc::from_text(base).encode_state();
        let merged = merge_by_component(Some(&base_state), ours, theirs).unwrap();

        let queue_body = merged
            .split("<!-- agent:queue -->")
            .nth(1)
            .and_then(|s| s.split("<!-- /agent:queue -->").next())
            .unwrap();
        assert!(
            queue_body.contains("~~do [#abcd] fix the thing~~"),
            "struck head must stay struck:\n{queue_body}"
        );
        let live = queue_body
            .lines()
            .filter(|l| l.contains("do [#abcd]") && !l.contains("~~"))
            .count();
        assert_eq!(
            live, 0,
            "stale un-strike resurrected the struck head LIVE:\n{queue_body}"
        );
    }

    /// Free-text variant of the resurrection: same residue shape, but the head is
    /// an operator free-text line (no `#id`), so the fix must also reconcile
    /// free-text identities (`#qdedupsync` was free-text-blind).
    #[test]
    fn merge_by_component_struck_free_text_head_not_resurrected_by_stale_unstrike() {
        let base = "<!-- agent:queue -->\n- ~~Still getting JB File Cache Conflict dialogs.~~\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- Still getting JB File Cache Conflict dialogs.\n<!-- /agent:queue -->\n";
        let theirs = "<!-- agent:queue -->\n- ~~Still getting JB File Cache Conflict dialogs.~~\n<!-- /agent:queue -->\n";

        let base_state = CrdtDoc::from_text(base).encode_state();
        let merged = merge_by_component(Some(&base_state), ours, theirs).unwrap();

        let queue_body = merged
            .split("<!-- agent:queue -->")
            .nth(1)
            .and_then(|s| s.split("<!-- /agent:queue -->").next())
            .unwrap();
        assert!(
            queue_body.contains("~~Still getting JB File Cache Conflict dialogs.~~"),
            "struck free-text head must stay struck:\n{queue_body}"
        );
        let live = queue_body
            .lines()
            .filter(|l| {
                l.contains("Still getting JB File Cache Conflict dialogs.") && !l.contains("~~")
            })
            .count();
        assert_eq!(
            live, 0,
            "stale un-strike resurrected the struck free-text head LIVE:\n{queue_body}"
        );
    }

    /// Multi-cycle no-churn proof (`#qheadresidue`): strike a head, persist the
    /// struck state as the next cycle's base, then re-merge with a stale un-strike
    /// agent side every cycle. The head must stay struck across every cycle with
    /// no re-emit churn (struck stays struck, never re-added live then re-struck).
    #[test]
    fn merge_by_component_struck_head_stable_across_cycles_no_churn() {
        // Cycle 0 starts from a struck base.
        let mut base =
            "<!-- agent:queue -->\n- ~~do [#abcd] answered~~\n<!-- /agent:queue -->\n".to_string();
        for cycle in 0..3 {
            let base_state = CrdtDoc::from_text(&base).encode_state();
            // Agent side keeps regenerating the head LIVE (stale un-strike).
            let ours = "<!-- agent:queue -->\n- do [#abcd] answered\n<!-- /agent:queue -->\n";
            // Disk stays correctly struck.
            let theirs = "<!-- agent:queue -->\n- ~~do [#abcd] answered~~\n<!-- /agent:queue -->\n";
            let merged = merge_by_component(Some(&base_state), ours, theirs).unwrap();
            let queue_body = merged
                .split("<!-- agent:queue -->")
                .nth(1)
                .and_then(|s| s.split("<!-- /agent:queue -->").next())
                .unwrap();
            let live = queue_body
                .lines()
                .filter(|l| l.contains("do [#abcd]") && !l.contains("~~"))
                .count();
            assert_eq!(
                live, 0,
                "cycle {cycle}: head resurrected LIVE (churn):\n{queue_body}"
            );
            assert!(
                queue_body.contains("~~do [#abcd] answered~~"),
                "cycle {cycle}: struck head lost:\n{queue_body}"
            );
            assert_eq!(
                queue_body.matches("do [#abcd] answered").count(),
                1,
                "cycle {cycle}: head duplicated:\n{queue_body}"
            );
            // Persist the merged (lawful, struck) state as the next cycle's base.
            base = merged;
        }
    }

    // ---- #qcellmerge1: cross-cell contamination is impossible by construction --

    /// Build a 3-component template doc (`status`, `exchange`, `queue`) for the
    /// multi-component concurrent-edit case.
    fn doc_with_status_exchange_queue(status: &str, exchange: &str, queue: &str) -> String {
        format!(
            "---\nagent_doc_format: template\n---\n\n## Status\n\n<!-- agent:status -->\n{status}\n<!-- /agent:status -->\n\n## Exchange\n\n<!-- agent:exchange -->\n{exchange}\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n{queue}\n<!-- /agent:queue -->\n"
        )
    }

    fn body_of<'a>(merged: &'a str, name: &str) -> &'a str {
        let open = format!("<!-- agent:{name} -->");
        let close = format!("<!-- /agent:{name} -->");
        merged
            .split(&open)
            .nth(1)
            .and_then(|s| s.split(&close).next())
            .unwrap_or("")
    }

    /// The headline `#qcellmerge1` invariant, asserted as one test in all three
    /// required directions: forward, inverse, and a multi-component concurrent
    /// edit. Each `agent:*` component is its own CRDT merge unit, so content from
    /// one component can NEVER splice into another — the live 2026-06-20
    /// console-output-in-`agent:queue` corruption class is unreproducible by
    /// construction.
    #[test]
    fn cross_cell_contamination_is_impossible() {
        let console = "```\n● Supervisor is now fresh (recycled to 0.34.35).\n```";

        // --- Forward: extra content in A (exchange) must NOT leak into B (queue).
        // The documented real failure shape: an `agent:exchange` console block
        // must never appear inside `agent:queue`. The agent appends the console
        // block to `exchange` while the operator concurrently types in `queue`.
        {
            let base = doc_with_exchange_queue("Q.", "- operator typing");
            let base_state = CrdtDoc::from_text(&base).encode_state();
            let ours = doc_with_exchange_queue(
                &format!("Q.\n\n### Re: q — opus\n\n{console}"),
                "- operator typing",
            );
            let theirs = doc_with_exchange_queue("Q.", "- operator typing more");

            let merged = merge_by_component(Some(&base_state), &ours, &theirs).unwrap();
            assert!(
                !body_of(&merged, "queue").contains("Supervisor is now fresh"),
                "FORWARD: exchange console block spliced into queue:\n{}",
                body_of(&merged, "queue")
            );
            // Both real edits survived, in their own cells.
            assert!(
                body_of(&merged, "exchange").contains("Supervisor is now fresh"),
                "FORWARD: console block lost from its own (exchange) cell:\n{merged}"
            );
            assert!(
                body_of(&merged, "queue").contains("typing more"),
                "FORWARD: operator queue edit lost:\n{merged}"
            );
        }

        // --- Inverse: extra content in B (queue) must NOT leak into A (exchange).
        // The operator pastes a multi-line fenced block into a `queue` item while
        // the agent concurrently appends a `### Re:` block to `exchange`. The
        // queue paste must never appear inside the exchange cell.
        {
            let queue_paste = "- here is my log:\n  ```\n  panic at line 9000\n  ```";
            let base = doc_with_exchange_queue("Q.", "- placeholder");
            let base_state = CrdtDoc::from_text(&base).encode_state();
            let ours =
                doc_with_exchange_queue("Q.\n\n### Re: q — opus\n\nAgent answer.", "- placeholder");
            let theirs = doc_with_exchange_queue("Q.", queue_paste);

            let merged = merge_by_component(Some(&base_state), &ours, &theirs).unwrap();
            assert!(
                !body_of(&merged, "exchange").contains("panic at line 9000"),
                "INVERSE: queue paste spliced into exchange:\n{}",
                body_of(&merged, "exchange")
            );
            assert!(
                body_of(&merged, "queue").contains("panic at line 9000"),
                "INVERSE: queue paste lost from its own cell:\n{merged}"
            );
            assert!(
                body_of(&merged, "exchange").contains("Agent answer."),
                "INVERSE: agent exchange edit lost:\n{merged}"
            );
        }

        // --- Multi-component concurrent edit: three cells (status, exchange,
        // queue) edited concurrently across ours/theirs. Each cell keeps exactly
        // its own edit and nothing crosses a cell boundary.
        {
            let base = doc_with_status_exchange_queue("idle", "Q.", "- do [#a1]");
            let base_state = CrdtDoc::from_text(&base).encode_state();
            // ours: agent flips status + appends an exchange response block.
            let ours = doc_with_status_exchange_queue(
                "running OURS_STATUS",
                &format!("Q.\n\n### Re: q — opus\n\n{console}"),
                "- do [#a1]",
            );
            // theirs: operator adds a queue item (and leaves status/exchange alone).
            let theirs =
                doc_with_status_exchange_queue("idle", "Q.", "- do [#a1]\n- do [#THEIRS_ITEM]");

            let merged = merge_by_component(Some(&base_state), &ours, &theirs).unwrap();

            let status_body = body_of(&merged, "status");
            let exchange_body = body_of(&merged, "exchange");
            let queue_body = body_of(&merged, "queue");

            // Each edit landed in its own cell.
            assert!(
                status_body.contains("OURS_STATUS"),
                "MULTI: status edit lost:\n{merged}"
            );
            assert!(
                exchange_body.contains("Supervisor is now fresh"),
                "MULTI: exchange edit lost:\n{merged}"
            );
            assert!(
                queue_body.contains("[#THEIRS_ITEM]"),
                "MULTI: queue edit lost:\n{merged}"
            );

            // Nothing crossed a cell boundary, in any direction.
            assert!(
                !status_body.contains("Supervisor is now fresh")
                    && !status_body.contains("[#THEIRS_ITEM]"),
                "MULTI: foreign content spliced into status:\n{status_body}"
            );
            assert!(
                !exchange_body.contains("OURS_STATUS") && !exchange_body.contains("[#THEIRS_ITEM]"),
                "MULTI: foreign content spliced into exchange:\n{exchange_body}"
            );
            assert!(
                !queue_body.contains("Supervisor is now fresh")
                    && !queue_body.contains("OURS_STATUS"),
                "MULTI: foreign content spliced into queue:\n{queue_body}"
            );
        }
    }
}
