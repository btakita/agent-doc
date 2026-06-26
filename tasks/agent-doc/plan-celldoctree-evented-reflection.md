# Plan: CellDocTree as a thin reactive-document adapter over lazily — evented reflection

## Architecture principle (operator, 2026-06-26)

Operator question: *"Is CellDocTree too specific to documents for lazily-rs?"*

**Answer: No — `CellDocTree` is correctly document-specific and belongs in `agent-doc`.**
lazily-rs already exposes the full **generic** reactive substrate, domain-agnostic:

- `CellTree` (`#lzordtree`) — ordered keyed reactive tree (`insert_child`/`remove_child`/`move_child`/…).
- `reconcile` + `DiffOp` + **`apply_to_map`** (`#lzkeyrecon`) — keyed diff → **incremental apply** to a `CellMap`.
- `SemTree` (`#lzsemtree`) — **memoized semantic tree** over the cell tree (per-node, subtree-scoped recompute).
- `SeqCrdt`, `TextCrdt`, `CellMap`/`CellFamily`, `CrdtPlaneRuntime`.

The only parts that are genuinely document-specific — and must **not** leak into lazily — are:
the markdown **projection** (`project_document` → component occurrences / items), the
`component:occurrence:item-id:index` **key scheme**, and the agent-doc **semantic queries**
(drainable heads, unresolved prompts, section summaries). `CellDocTree` is the thin adapter
that holds those three and drives lazily's generic primitives.

## The layering smell to fix

`CellDocTree::apply` (`agent-doc-core/src/cell_doc.rs`) currently **hand-rolls** the map-half of
what lazily's `apply_to_map` already does — its `Remove`/`Insert`/`Move`/`Update` arms are
line-for-line `apply_to_map`, plus parallel `CellTree` maintenance. So the increment is **not**
"build evented-reflection machinery in `CellDocTree`" (that would reinvent `apply_to_map` +
`SemTree`); it is "make `CellDocTree` delegate to lazily and stay thin." **Zero new generic
machinery in agent-doc.** Any genuinely-generic gap goes to lazily-rs.

## Scope boundary (READ FIRST)

This plan is **additive reactive-model groundwork**, default-off / not wired to change live
behavior. It does **NOT** rewire the live finalize/merge/write path. Wiring the evented model
into the live commit path (replacing whole-tree reconcile at finalize with per-node deltas) is
a **separate future rung** (`#cdtcutover`) and is explicitly out of scope here. Each phase is
additive: new/refactored code + tests, existing behavior byte-identical, `make check` +
`cargo build --release` green, isolated worktree, **no install/push**.

## Phases (dependency-ordered; one backlog id per actionable phase)

### `#cdtdedup` — delegate the map-half to lazily `apply_to_map`
`CellDocTree::apply` calls `lazily::apply_to_map(ctx, map, &diff.ops)` for the `CellMap` half
instead of the hand-rolled match; keep only the document-structure `CellTree` (`occ_node`)
maintenance on top. Behavior identical; removes the duplication.
- **Acceptance:** `apply` produces the same map/tree state as today for Insert/Remove/Move/Update;
  existing `CellDocTree` tests stay green; the hand-rolled map match is gone.

### `#lzapplytree` — [lazily-rs] generic `apply_to_tree` companion to `apply_to_map`
The `CellTree`-half maintenance in `CellDocTree::apply` (apply a `&[DiffOp]` to a `CellTree`'s
ordered children: remove/insert+move/move/set) is **generic**, not document-specific — it is the
tree analogue of `apply_to_map`. Add `lazily::apply_to_tree(ctx, &CellTree, &[DiffOp])` in
lazily-rs so `CellDocTree::apply` delegates **both** halves and carries zero reconciliation logic.
- **Cross-repo:** lands in `src/lazily-rs` (its own `make check`/release + release gating), then
  `#cdtdedup`'s tree-half delegates to it. Sequenceable as a fast-follow to `#cdtdedup` (which can
  first delegate only the map-half).
- **Acceptance:** `apply_to_tree` round-trips a reconcile op set onto a `CellTree` (moves use
  `move_child`, only changed values written); unit-tested in lazily-rs.

### `#cdtupdate` — evented `CellDocTree::update_to(old_doc, new_doc)` (the commit phase)
Add `CellDocTree::update_to(ctx, old_doc, new_doc)` that runs `diff_document(old, new)` and
applies every `ComponentDiff` incrementally (via `apply`), instead of `from_document` rebuild —
push per-node deltas into the model on write. Handle component add/remove (new/dropped occurrence
nodes) and multi-component revisions.
- **Depends on:** `#cdtdedup`.
- **Acceptance:** for a sequence of revisions, `update_to`-driven tree is **structurally identical**
  (items/values/order per occurrence, component set) to `from_document(new)` — covering Insert,
  Remove, Update, Move, and multi-component edits.

### `#cdtsem` — agent-doc semantic queries as `SemTree` memoized computeds
Express an agent-doc semantic query (start with drainable-head / unresolved-prompt identities per
component, or per-component item counts) as a `lazily::SemTree` computed over the `CellDocTree`,
so editing an item in component A does **not** recompute component B's query (subtree-scoped, lazy).
- **Depends on:** `#cdtupdate`.
- **Acceptance:** subtree-scoped recompute proven via a recompute probe (editing component A's item
  recomputes only A's computed, not B's); a memo-equality guard (re-read without change ⇒ no
  recompute); the computed's value matches the eager query.

### `#cdtcutover` — [FUTURE, SEPARATE RUNG — not this increment]
Wire the evented model into the live finalize/write path (replace whole-tree reconcile at finalize
with `update_to` per-node deltas; expose the `SemTree` queries to the supervisor/preflight drainable
computations). Large, live-supervisor-critical, default-gated, needs its own focused cycle + operator
live proof. **Do not start under this plan.**

## Sequencing

`#cdtdedup` first (bounded refactor). `#lzapplytree` is the lazily-rs companion that lets the
tree-half also delegate (fast-follow; cross-repo). `#cdtupdate` adds the evented revision update.
`#cdtsem` demonstrates the memoized lazy-pull win. `#cdtcutover` is a separate future rung, gated.
