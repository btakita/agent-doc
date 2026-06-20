# Plan: Per-cell (per-component) CRDT merge isolation — `#qcellmerge`

## Problem

`agent-doc-core/src/crdt.rs::merge(base_state, ours_text, theirs_text)` reconciles the
**entire document as one Yrs `Text` blob**. Every write/finalize/convergence merges the
whole file, so:

- Concurrent edits to *unrelated* regions (operator typing a queue item while the agent
  writes an `exchange` response) compete in **one** merge → spurious drift, duplicate
  insertions, and stalls. The operator's stated case: "I was typing into this queue item
  so it should not have any effect on the running job and it should not stall the queue."
- **Cross-component corruption**: unrelated content can splice into the wrong component.
  Live repro 2026-06-20: agent **console output** ("● Supervisor is now fresh …") was
  merged INTO the `agent:queue` component as a fenced block while the operator was typing
  a prompt. A whole-document text merge has no notion that the queue and the console are
  different cells.
- The `dedup_adjacent_blocks` pass and the `<50% common prefix → replace base` heuristic
  in `merge()` are cleanup hacks for duplicate insertions this blob-merge produces.

Operator directive (2026-06-20): "We need to isolate queue items as cells for content
merges. Not an entire document CRDT merge … to prevent merge corrupting the editor buffer
events." Plus: "Any changes made by agent-doc should immediately be reflected in the
document model" (reactive/evented reflection — tracked separately, see Phase 4).

## Design

**Generalized goal (operator, 2026-06-20):** "Generalize cell merge … applied to the
entire structure of the document AST. So if someone writes a prompt interleaved with an
exchange response, the cell is the particular exchange response. The key is isolation of
merges, to avoid conflicts and drift. The React Virtual DOM may be a good example of what
we are trying to do."

So the cell is **not** fixed at "component" — **a cell is any AST node**, and isolation
applies recursively at every structural level. The merge reconciles **by node identity**,
exactly like React's Virtual DOM diff: each node carries a durable key, the reconciler
matches ours/theirs/base nodes by key, merges only matched pairs against their own base, and
treats an unmatched node as a pure insert/delete (never text contention). Two edits that
touch different keyed nodes can *never* contend, regardless of nesting depth, because they
reconcile in separate sub-trees.

Map onto the existing structure (coarse → fine; each level is the **same** keyed-reconcile
algorithm applied to a finer AST):
1. **Component-level** (Phase 1, **shipped** `#qcellmerge1`): `exchange`, `queue`,
   `backlog`, `review`, `status`, `done` each merge independently, keyed by component name.
   Prevents cross-component splicing (the console-in-queue corruption) and lets an
   `exchange` write and a `queue` edit proceed without contending. This is the coarsest
   "virtual DOM level" — components are the top-level keyed children.
2. **Durable per-cell base** (Phase 2): each cell carries its own stable merge base across
   cycles instead of one whole-doc base, so a node's base advances independently.
3. **Recursive AST-node cells** (Phase 3, generalized): drill the same keyed reconciliation
   *inside* each component. In `queue`/`backlog` the keyed nodes are list items (by the
   durable node key the strike paths already use). In `exchange` the keyed nodes are the
   `### Re:` response blocks (key = heading/prompt identity) plus interleaved user prompts —
   so an operator prompt typed *into* an exchange while the agent writes a *different*
   `### Re:` block reconciles as two distinct keyed nodes and never contends. This is the
   operator's literal "the cell is the particular exchange response".
4. **Evented reflection** (Phase 4): push per-cell deltas into the model on write (React's
   "commit phase") instead of reconciling the whole tree at finalize.

The merge layer is the last place that still operates on full text; `component::parse`
already gives the top-level keyed children, and the granular `--pending-*` write flags
already mutate exactly one node. Phase 3 generalizes node identity below the component.

## Phases

### `#qcellmerge1` — Component-scoped merge (the anti-corruption rung)
- Add `merge_by_component(base_state, ours_text, theirs_text)` that:
  1. `component::parse` ours and theirs (and decode base) into named regions.
  2. For each component name present, run the existing `crdt::merge` on **just that
     component's** base/ours/theirs slices.
  3. Recombine merged components in document order; non-component preamble/interstitial
     text merges as its own residual cell.
- Structural divergence (a component added/removed on one side) falls back to the current
  whole-doc merge for that region only, logged — never silently.
- Route `merge.rs` / the write/finalize convergence path through `merge_by_component`.
- **Acceptance:** a SimWorld case where side A appends to `exchange` and side B appends a
  `queue` item converges with BOTH changes and ZERO cross-component content; the
  console-output-in-queue corruption class is unreproducible. Existing `crdt`/`merge`
  tests stay green (component merge degenerates to whole-doc merge for a single-component
  doc).

### `#qcellmerge2` — Per-component CRDT state persistence
- Persist `.agent-doc/crdt/<hash>.<component>.yrs` per component (or a structured
  multi-cell state in one file) so each cell carries its own stable base across cycles,
  instead of one whole-doc `<hash>.yrs`. Migration: decode the existing whole-doc state and
  split, or rebuild per-component lazily.
- **Acceptance:** round-trip per-component encode/decode; a cell's base advances
  independently; compaction GCs per cell.

### `#qcellmerge3` — Recursive AST-node cells (keyed reconciliation, generalized)
- Drill the Phase 1 keyed reconciliation *inside* each component, so the cell is an AST
  node at any depth (the operator's "entire structure of the document AST"):
  - `queue`/`backlog`: each list item is a cell, keyed by the durable node key the strike
    paths already use — two concurrent edits to different items never contend and an
    in-progress item edit cannot stall the queue ("isolate queue items as cells").
  - `exchange`: each `### Re:` response block is a cell, keyed by heading/prompt identity,
    with interleaved user prompts as their own nodes — a prompt typed into the exchange
    while the agent writes a *different* response block reconciles as two distinct cells
    ("the cell is the particular exchange response").
- Engine: a single recursive `reconcile(base_node, ours_node, theirs_node)` that matches
  children by key (React-VDOM style), recurses on matched pairs, and treats key-only-on-
  one-side children as insert/delete. Component-level merge becomes the top call of this
  recursion; whole-doc text merge stays the leaf for an unkeyed/opaque node.
- **Acceptance:** SimWorld — (a) editing queue item B while item A is the running head does
  not touch A and does not stall the drain; (b) a user prompt inserted between two committed
  `### Re:` blocks while the agent appends a new block converges with all three present and
  zero cross-block splice.

### `#qcellmerge4` — Evented reflection (links to the drift answer's Phase 2)
- "Changes made by agent-doc should immediately be reflected in the document model":
  push agent-side deltas into the per-cell model on write rather than reconciling at
  finalize. Couples with the `DocumentListener` / `onDidChangeTextDocument` evented-IPC
  direction from the drift analysis. Larger; sequence after 1–3 prove out.

## Sequencing

`#qcellmerge1` first — it is the highest-leverage anti-corruption rung and is bounded to
the merge layer. `2` makes the isolation durable across cycles. `3` delivers the
operator's literal "queue items as cells". `4` is the evented-reflection follow-on.

## Risk

The merge layer is load-bearing for every write; a half-correct change worsens corruption.
Each phase ships behind full SimWorld convergence coverage and keeps the whole-doc merge as
the structural-divergence fallback. Implement in a focused cycle with a clean context, not
mid-drift.
