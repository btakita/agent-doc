# Plan: Per-node (per-component) CRDT merge isolation — `#qnodemerge`

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
  different nodes.
- The `dedup_adjacent_blocks` pass and the `<50% common prefix → replace base` heuristic
  in `merge()` are cleanup hacks for duplicate insertions this blob-merge produces.

Operator directive (2026-06-20): "We need to isolate queue items as nodes for content
merges. Not an entire document CRDT merge … to prevent merge corrupting the editor buffer
events." Plus: "Any changes made by agent-doc should immediately be reflected in the
document model" (reactive/evented reflection — tracked separately, see Phase 4).

## Design

**Generalized goal (operator, 2026-06-20):** "Generalize node merge … applied to the
entire structure of the document AST. So if someone writes a prompt interleaved with an
exchange response, the node is the particular exchange response. The key is isolation of
merges, to avoid conflicts and drift. The React Virtual DOM may be a good example of what
we are trying to do."

So the node is **not** fixed at "component" — **a node is any AST node**, and isolation
applies recursively at every structural level. The merge reconciles **by node identity**,
exactly like React's Virtual DOM diff: each node carries a durable key, the reconciler
matches ours/theirs/base nodes by key, merges only matched pairs against their own base, and
treats an unmatched node as a pure insert/delete (never text contention). Two edits that
touch different keyed nodes can *never* contend, regardless of nesting depth, because they
reconcile in separate sub-trees.

Map onto the existing structure (coarse → fine; each level is the **same** keyed-reconcile
algorithm applied to a finer AST):
1. **Component-level** (Phase 1, **shipped** `#qnodemerge1`): `exchange`, `queue`,
   `backlog`, `review`, `status`, `done` each merge independently, keyed by component name.
   Prevents cross-component splicing (the console-in-queue corruption) and lets an
   `exchange` write and a `queue` edit proceed without contending. This is the coarsest
   "virtual DOM level" — components are the top-level keyed children.
2. **Durable per-node base** (Phase 2): each node carries its own stable merge base across
   cycles instead of one whole-doc base, so a node's base advances independently.
3. **Recursive AST-node nodes** (Phase 3, generalized): drill the same keyed reconciliation
   *inside* each component. In `queue`/`backlog` the keyed nodes are list items (by the
   durable node key the strike paths already use). In `exchange` the keyed nodes are the
   `### Re:` response blocks (key = heading/prompt identity) plus interleaved user prompts —
   so an operator prompt typed *into* an exchange while the agent writes a *different*
   `### Re:` block reconciles as two distinct keyed nodes and never contends. This is the
   operator's literal "the node is the particular exchange response".
4. **Evented reflection** (Phase 4): push per-node deltas into the model on write (React's
   "commit phase") instead of reconciling the whole tree at finalize.

The merge layer is the last place that still operates on full text; `component::parse`
already gives the top-level keyed children, and the granular `--pending-*` write flags
already mutate exactly one node. Phase 3 generalizes node identity below the component.

## Phases

### `#qnodemerge1` — Component-scoped merge (the anti-corruption rung)
- Add `merge_by_component(base_state, ours_text, theirs_text)` that:
  1. `component::parse` ours and theirs (and decode base) into named regions.
  2. For each component name present, run the existing `crdt::merge` on **just that
     component's** base/ours/theirs slices.
  3. Recombine merged components in document order; non-component preamble/interstitial
     text merges as its own residual node.
- Structural divergence (a component added/removed on one side) falls back to the current
  whole-doc merge for that region only, logged — never silently.
- Route `merge.rs` / the write/finalize convergence path through `merge_by_component`.
- **Acceptance:** a SimWorld case where side A appends to `exchange` and side B appends a
  `queue` item converges with BOTH changes and ZERO cross-component content; the
  console-output-in-queue corruption class is unreproducible. Existing `crdt`/`merge`
  tests stay green (component merge degenerates to whole-doc merge for a single-component
  doc).

### `#qnodemerge2` — Per-component CRDT state persistence
- Persist `.agent-doc/crdt/<hash>.<component>.yrs` per component (or a structured
  multi-node state in one file) so each node carries its own stable base across cycles,
  instead of one whole-doc `<hash>.yrs`. Migration: decode the existing whole-doc state and
  split, or rebuild per-component lazily.
- **Acceptance:** round-trip per-component encode/decode; a node's base advances
  independently; compaction GCs per node.

### `#qnodemerge3` — Recursive AST-node nodes (keyed reconciliation, generalized)
- Drill the Phase 1 keyed reconciliation *inside* each component, so the node is an AST
  node at any depth (the operator's "entire structure of the document AST"):
  - `queue`/`backlog`: each list item is a node, keyed by the durable node key the strike
    paths already use — two concurrent edits to different items never contend and an
    in-progress item edit cannot stall the queue ("isolate queue items as nodes").
  - `exchange`: each `### Re:` response block is a node, keyed by heading/prompt identity,
    with interleaved user prompts as their own nodes — a prompt typed into the exchange
    while the agent writes a *different* response block reconciles as two distinct nodes
    ("the node is the particular exchange response").
- Engine: a single recursive `reconcile(base_node, ours_node, theirs_node)` that matches
  children by key (React-VDOM style), recurses on matched pairs, and treats key-only-on-
  one-side children as insert/delete. Component-level merge becomes the top call of this
  recursion; whole-doc text merge stays the leaf for an unkeyed/opaque node.
- **Acceptance:** SimWorld — (a) editing queue item B while item A is the running head does
  not touch A and does not stall the drain; (b) a user prompt inserted between two committed
  `### Re:` blocks while the agent appends a new block converges with all three present and
  zero cross-block splice.

### `#qnodemerge4` — Op-capture / evented reflection (accuracy lever 1: highest leverage)
- Stop reconstructing edits from a text diff; capture the editor's **real operations**
  (`DocumentListener.documentChanged` / `onDidChangeTextDocument` — insert@offset,
  delete-range) and feed those ops to the per-node Yrs model directly. This removes the
  diff-guess (Myers diff picks *a* edit script, not necessarily the one the user performed),
  so live-edit merges become exact/intention-preserving at the character level. Also pushes
  agent-side deltas into the model on write rather than reconciling at finalize.
- **Acceptance:** a concurrent live edit + agent write converges with zero diff-derived
  duplication; ops replay equals editor-observed state.

### `#qnodemerge5` — Surface true conflicts, never fabricate (accuracy lever 5: the safety net)
- For the irreducible remainder — genuine concurrent edits to the **same** leaf node, where
  the merge is information-theoretically underdetermined — do **not** silently auto-merge a
  text neither side wrote. Surface both versions (note it in `agent:exchange` / a conflict
  marker) and let the operator resolve. 100% accuracy by refusing to guess (extends the
  `#hap7` "note structural conflicts in exchange" idea).
- **Acceptance:** a true same-leaf concurrent edit produces an operator-visible conflict, not
  a fabricated merge; non-conflicting edits never trip it.

## Accuracy levers → phases (operator's "five concrete accuracy levers", priority order)

The keyed-diff approach reaches ~100% practical accuracy via five levers, mapped to phases
and ordered by implementation priority (each depends on the prior structural layer; op-capture
is the highest *leverage* but needs the nodes under it first):

1. **Correct per-node base** (lever 3) → `#qnodemerge2` — removes the stale-base duplication class.
2. **Finer AST nodes** (lever 2) + **first-class moves** (lever 4) → `#qnodemerge3` — shrinks the
   same-leaf surface; keyed identity treats a moved block as a move, not delete+insert.
3. **Op-capture / evented** (lever 1, highest leverage) → `#qnodemerge4` — removes diff-guessing entirely.
4. **Surface conflicts, never fabricate** (lever 5) → `#qnodemerge5` — honest handling of the
   irreducible same-leaf remainder.

## Sequencing

`#qnodemerge1` first (shipped) — the anti-corruption rung, bounded to the merge layer. `2`
makes the isolation durable across cycles (correct base). `3` delivers recursive AST nodes +
moves. `4` is the highest-leverage accuracy win (op-capture) but depends on 2+3. `5` is the
conflict-surfacing safety net, sequenceable any time after `3`.

## Risk

The merge layer is load-bearing for every write; a half-correct change worsens corruption.
Each phase ships behind full SimWorld convergence coverage and keeps the whole-doc merge as
the structural-divergence fallback. Implement in a focused cycle with a clean context, not
mid-drift.
