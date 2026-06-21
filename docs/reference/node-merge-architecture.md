# Document Node-Merge Architecture

How agent-doc merges concurrent edits (agent writes vs. live editor keystrokes) without
splicing content across unrelated regions of the document.

> **Companion read:** [Full-Document IPC Corruption Chain](full-document-ipc-corruption-chain.md)
> describes the failure mode this architecture exists to eliminate.

> **Maintenance:** This document tracks the merge engine in
> [`agent-doc-core/src/crdt.rs`](../../agent-doc-core/src/crdt.rs). When the merge logic
> changes (`merge`, `merge_by_component`, `segment_into_cells`, or the roadmap phases
> below), update this file in the same change.

## The problem: whole-document blob merge

The original merge (`crdt::merge`) treats the **entire document as one Yrs `Text` blob**.
Every write / finalize / convergence threw the whole file into a single three-way merge
(`base`, `ours`, `theirs`). Because the merge had no notion of document structure, a queue
keystroke and an agent `exchange` write competed inside the *same* merge even though they
touch unrelated regions — and text could land in the wrong region.

**Live repro:** agent console output (`● Supervisor is now fresh …`) got merged **into** the
`agent:queue` component as a fenced block while the operator was typing a prompt. The
stale-base-detection and `dedup_adjacent_blocks` hacks were band-aids over duplicates that
blob-merging itself produced.

## The node model

A **node** is an isolated merge unit. The document is segmented into nodes, and each node
merges only against its own prior state — two nodes never share a merge, so content can
never splice from one into another.

The coarsest level of this model is **per-component**: each `<!-- agent:* -->` block
(`exchange`, `queue`, `backlog`, `review`, `done`, …) is a node, with the text between
components ("interstitials") paired positionally. The roadmap generalizes the same idea
recursively down to individual list items and `### Re:` blocks.

> **Code vs. prose terminology:** the concept is called a **node**. The current code symbol
> is `Cell` (`segment_into_cells`, `Cell::Component`, `Cell::Interstitial`) — a symbol rename
> to match the prose is tracked separately. Read "cell" in the code as "node."

## Current implementation — `merge_by_component`

Shipped as the anti-corruption rung (`#qnodemerge1`). Entry point:
`crdt::merge_by_component(base_state, ours_text, theirs_text)`. Both FFI merge entry points
route through it.

1. **Short-circuit** — if `ours == theirs`, return as-is.
2. **Segment** both `ours` and `theirs` into nodes via `segment_into_cells`. If either fails
   to segment, fall back to the whole-doc `merge` (logged).
3. **Inline-mode guard** — if neither side has any components (a component-less / inline
   document), delegate to the legacy whole-doc `merge` with the original state, preserving
   exact prior behavior.
4. **Structural-divergence guard** — if the *set or order* of component names differs between
   `ours` and `theirs`, a per-node pairing is unsound, so fall back to the whole-doc `merge`
   (logged). Structural reconciliation across differing node sets is the job of the recursive
   phase (`#qnodemerge3`), not this rung.
5. **Per-node base alignment** — decode the base state once, segment it, and build a
   `name → content` map (`base_by_name`) plus a positional list of interstitial base slots.
   Each node resolves its own base: components by name (so the `exchange` committed-response
   guard sees its *real* base), interstitials by position.
6. **Per-node merge** — walk the `ours`/`theirs` node pairs in document order. If a pair is
   identical, keep it verbatim; otherwise run the three-way leaf `merge` against *that node's*
   base only.
7. **Recombine** in document order.

The leaf merge is still the whole-doc `merge`, now applied to one node's text at a time.

## The leaf merge — `merge`

Three-way CRDT merge over text using three Yrs actors (`base`, `ours`, `theirs`): apply each
side's diff-from-base, merge the updates, return the conflict-free result. Two safeguards
matter for correctness:

- **Stale-base detection** — if the base text shares too little with both sides (checked via
  common prefix *and* suffix, since template documents bookend the exchange with structural
  frontmatter / markers / pending sections), the base is treated as stale and `ours` is used
  as the base to prevent duplicate insertions.
- **Committed-response preservation** (`#ipc-crdt-response-drift`) — committed `### Re:`
  blocks are append-only history. The merge captures committed response headings from the
  *original* base before stale-base advancement can rewrite it, so a stale or divergent
  `theirs` can never delete a committed response out of the merged result. Boundary markers
  (`<!-- agent:boundary:… -->`) and working-tree-only ` (HEAD)` annotations are treated as
  transient and never count as new content.

## Durable per-node base — `MultiNodeState` (`#qnodemerge2`)

`merge_by_component` derives each node's base by decoding *one* whole-doc state and slicing it
by name — so the persisted base (`<hash>.yrs`) is still a single blob whose Yrs clock is shared
across every node. `MultiNodeState` makes the base durable *per node*:

- **One Yrs state per node, one file.** `MultiNodeState::from_text` segments the document into
  top-level nodes (components + interstitials) and encodes each node's text as its own Yrs
  state. `encode`/`decode` round-trip the whole set into a single self-describing container
  (`MAGIC | version | count | [name, state]…`) persisted at `.agent-doc/crdt/<hash>.nodes.yrs`.
- **Deterministic encoding.** Node states use a fixed Yrs client id (`encode_text_deterministic`)
  so identical text always re-encodes to byte-identical bytes — an untouched node's base is
  provably unchanged across a cycle, and there are no spurious sidecar rewrites. The base client
  id is irrelevant to the leaf `merge` (which reads only the base text), so this is safe.
- **Independent advance.** `MultiNodeState::merge(base, ours, theirs)` runs the same per-node
  reconciliation as `merge_by_component` (shared `merge_aligned_nodes`), but resolves each node's
  base from *its own* persisted state and returns a fresh `MultiNodeState` where only changed
  nodes advanced. Structural divergence / component-less docs fall back to the whole-doc `merge`,
  same safety net as the component rung.
- **Migration & GC.** `snapshot::multinode_crdt_state` reads the `.nodes.yrs` sidecar, lazily
  migrating a legacy whole-doc `<hash>.yrs` (decode → split) when the sidecar is absent.
  `save_document_crdt` rebuilds and rewrites the sidecar every cycle (so compaction, which routes
  through it, GCs per node), `delete_crdt` removes it, and the rename migration carries it with
  the document. The legacy `<hash>.yrs` and `<hash>.overlay.yrs` are still written for
  back-compat; the live orchestration merge base is unchanged pending the recursive `#qnodemerge3`
  rung.

## Roadmap — recursive AST-node merge

`merge_by_component` is the component-level (coarsest) rung. The full model applies node
isolation to the **entire document AST**, reconciled by durable node identity the way a React
Virtual DOM keys its children.

| Phase | What it adds |
| --- | --- |
| `#qnodemerge1` ✅ shipped | Component-scoped merge (`merge_by_component`) — the anti-corruption rung above. |
| `#qnodemerge2` ✅ shipped | Per-node CRDT **state persistence** — `MultiNodeState` (`crdt.rs`) persists one independent Yrs state per top-level node into a single structured container (`<hash>.nodes.yrs`), the per-component successor to the whole-doc `<hash>.yrs`. Each node carries its own stable base across cycles (an untouched node re-encodes byte-identically; a changed node's base advances on its own). Migrates a legacy whole-doc `<hash>.yrs` lazily, rebuilds (GCs) per node every save/compaction, and follows the document across renames. See *Durable per-node base* below. |
| `#qnodemerge3` | **Recursive AST-node reconciliation** — one `reconcile(base, ours, theirs)` matching children by durable key and recursing on matched pairs. Queue/backlog items *and* `### Re:` blocks (with interleaved prompts) become nodes; the whole-doc text merge becomes the leaf. |
| `#qnodemerge4` | **Op-capture / evented reflection** (highest-leverage accuracy lever) — feed real editor operations (`DocumentListener.documentChanged` / `onDidChangeTextDocument`) into the per-node model instead of reconstructing edits from a text diff, removing the diff-guess entirely. |
| `#qnodemerge5` | **Surface true conflicts, never fabricate** — for a genuine concurrent edit to the *same* leaf node (information-theoretically underdetermined), present both versions for operator resolution instead of silently auto-merging text neither side wrote. |

Accuracy ordering is the dependency order: `2 → 3 → 4`, with `5` sequenceable any time after
`3`. The structural layer (keyed reconciliation) gets the document into the right *shape*; the
accuracy comes from feeding it real ops (`4`) on a correct per-node base (`2`/`3`) and
refusing to guess on genuine conflicts (`5`).
