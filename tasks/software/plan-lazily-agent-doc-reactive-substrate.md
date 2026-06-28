# Plan: lazily-rs as the agent-doc reactive-document substrate

**Goal:** ship the lazily-rs primitives that let agent-doc model a markdown document as a
**reactive, per-cell-mergeable, ordered keyed tree** — replacing the current whole-document
`crdt::merge` with minimal per-item ops, the long-standing per-cell CRDT-merge intent.

This plan sequences the building blocks identified across the lazily-rs design Q&A
(tree algorithm, large structural edits, "it's text — identity isn't inherent").
Each phase is an actionable backlog id; phases are dependency-ordered.

## Design recap (already established in the session)

agent-doc's document is a **shallow, ordered, stably-keyed tree**: root → components
(`exchange`, `queue`, `backlog`, `review`, …) in document order → items in order, each with a
stable `component:occurrence:item-id:index` node_key. The target architecture:

- **Structure** — ordered keyed tree: `(kind, stable_id, value_cell, ordered children)`, cheap
  `Rc` structural sharing between revisions. Children are an **ordered** collection (order is
  meaningful), not a hash map.
- **Diff** — **keyed reconciliation per level** with **LIS move-minimization** (Vue3/Inferno
  style): match by stable id, emit a minimal `{insert, remove, move, update}` op set per item.
  This is the enabling step for per-cell merge.
- **Order under concurrency** — a **move-aware sequence CRDT** (fractional indexing / RGA /
  Fugue) for mergeable sibling order without a coordinator.
- **Atomic move** — reconciliation *emits* moves; a move must preserve the node's cell identity,
  dependents, and CRDT lineage (not remove+insert). `#lzcellmove` is the keystone.
- **Identity for text** — markdown has no inherent node ids, so identity is *manufactured*:
  in-band anchors (agent-doc already has these) for the controlled skeleton; content-derived
  keys + alignment diff for free prose; a char/line text CRDT + re-parse for concurrent free text.
- **Two layers** — syntactic AST = input/value cells; semantic tree (unresolved prompts,
  drainable heads, section summaries) = **memoized computeds** over the relevant subtree.

## Phases (dependency-ordered)

### Phase 0 — `#lzcellmove` (KEYSTONE, prerequisite, already in review/operator-go)
Atomic + optimized ordered move/reorder for `CellMap`/`CellFamily`: reorder a key in place
(same entry cell + dependents, single membership bump) vs naive remove+insert. Likely a
`CellList<V>` ordered variant or `CellMap::move_to(key, index)`. Everything below depends on this.
Already tracked; this plan does not re-file it.

### Phase 1 — `#lzordtree`: ordered keyed tree primitive
Build the `CellTree` / ordered-children node on top of `CellList`/`#lzcellmove`:
`(kind, stable_id, value_cell, ordered children)` with `Rc` structural sharing. Per-node value
edits invalidate only that node's readers; per-level membership reactivity. This is the document
tree shape agent-doc components + items map onto. Plan-first; tests for per-node isolation,
ordered children, structural sharing across revisions.

### Phase 2 — `#lzkeyrecon`: keyed reconciliation with LIS move-minimization
Given old tree + new tree, diff each level **by stable id** and emit the minimal
`{insert, remove, move, update}` op set, using longest-increasing-subsequence over matched keys
to minimize moves. This is the algorithm that turns a structural edit into per-item ops instead
of a whole-subtree replace — **the direct per-cell-CRDT-merge enabler**. Tests: reorder→moves
(not replace), insert/remove, nested updates, move-count optimality vs naive.

### Phase 3 — `#lzseqcrdt`: move-aware sequence CRDT for sibling order
A mergeable order layer for children under concurrency: fractional indexing (simplest) or
RGA/Fugue, ideally move-aware (Kleppmann "moving elements in list CRDTs") so a reorder is one
move op, not delete+reinsert that duplicates under concurrent merge. Sits above the per-cell
value merge in `crdt.rs`. Tests: concurrent insert/reorder convergence, move-vs-move policy.

### Phase 4 — `#lzstableid`: manufactured identity for markdown text
The identity layer so reconciliation has keys to match on:
- anchored skeleton — honor in-band markers/item-ids (agent-doc already emits these);
- content-derived keys — hash normalized block content so an unchanged block keeps its key across
  reflow/reorder; edited block matched to predecessor by neighborhood + similarity (edit vs move);
- alignment diff (Myers/patience/histogram) for free prose runs.
Tests: stable id survives reorder/reindent, edit-vs-move classification, rewrite→replace honesty.

### Phase 5 — `#lzsemtree`: memoized semantic-tree computed layer
Derive the *semantic* document tree (unresolved prompts, drainable heads, section summaries) as
memoized `computed` nodes over the relevant subtree, recomputing only when that subtree's cells
change (lazy, glitch-free). Don't materialize semantics eagerly. Demonstrates the lazy-pull win
on a real agent-doc-shaped query. Tests: subtree-scoped recompute, memo equality guard.

### Phase 6 — `#lztextcrdt`: free-text char/line sequence CRDT + re-parse (heaviest, last)
For concurrent edits to arbitrary prose with no anchors: a char/line sequence CRDT (RGA / Yjs
text model) merges keystrokes, then **re-parse** the merged text and re-derive the structural
tree (tree = projection of CRDT-merged text). Honest floor: a true rewrite *is* a replace.
Least immediately needed (agent-doc managed components already merge per-component); ordered last.

## Notes / constraints

- Keep IPC Snapshot/Delta as the single-producer mirror; the sequence/text CRDTs live only at the
  multi-writer boundary — same split as `#lzcrdtplane` (distributed cell plane, separate review item).
- Do not adopt YRS/YJS document model wholesale (wrong replication unit for lazily's reactive
  graph + memo-equality invariants); optionally borrow YRS binary anti-entropy as transport only.
- Each phase: plan-first where it adds public API, `cargo fmt` + `clippy --all-features -D warnings`
  + tests + `make check`, then `#spec-test-build-install-commit-push`.
