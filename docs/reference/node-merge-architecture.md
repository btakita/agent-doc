# Document Node-Merge Architecture

How agent-doc merges concurrent edits (agent writes vs. live editor keystrokes) without
splicing content across unrelated regions of the document.

> **Companion read:** [Full-Document IPC Corruption Chain](full-document-ipc-corruption-chain.md)
> describes the failure mode this architecture exists to eliminate.

> **Maintenance:** This document tracks the merge engine in
> [`agent-doc-merge/src/lib.rs`](../../agent-doc-merge/src/lib.rs). When the merge logic
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

1. **Whole-document replay gate** — canonicalize an exact or monotonic duplicate projection
   on either side before any tree/CRDT reconciliation. If one side contains two structurally
   complete but divergent projections, reject the merge: the ordinary whole-document fallback
   is not allowed to concatenate an ambiguous replay. Monotonic comparison uses the shared
   code-block-aware ` (HEAD)` normalization and exchange-scoped prompt-prefix normalization;
   the retained projection remains byte-verbatim.
2. **Short-circuit** — if `ours == theirs`, return as-is.
3. **Segment** both `ours` and `theirs` into nodes via `segment_into_cells`. If either fails
   to segment, fall back to the whole-doc `merge` (logged).
4. **Inline-mode guard** — if neither side has any components (a component-less / inline
   document), delegate to the legacy whole-doc `merge` with the original state, preserving
   exact prior behavior.
5. **Structural-divergence guard** — if the *set or order* of component names differs between
   `ours` and `theirs`, a per-node pairing is unsound, so fall back to the whole-doc `merge`
   (logged), subject to the complete-document replay gate above. Structural reconciliation
   across differing node sets is the job of the recursive phase (`#qnodemerge3`), not this rung.
6. **Per-node base alignment** — decode the base state once, segment it, and build a
   `name → content` map (`base_by_name`) plus a positional list of interstitial base slots.
   Each node resolves its own base: components by name (so the `exchange` committed-response
   guard sees its *real* base), interstitials by position.
7. **Per-cell merge** — first project aligned components into keyed children and compose each
child against its own base with component ownership. Duplicate prompt keys receive stable
occurrence ordinals: the operator-owned side's exact multiplicity wins, so intentional
duplicates survive while retained-agent-only replay copies do not multiply.
8. **Component-local leaf fallback** — if one aligned component changes splittability (most
commonly a keyed response append racing a body-only live exchange), run the legacy leaf merge
for that component only. The mismatch must not demote unrelated queue/backlog components to a
flat CRDT merge with the original base.
9. **Recombine** in document order.

The leaf merge is still the whole-doc `merge`, applied to one component's text at a time.

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
- **Persistence & migration.** Production merge state is checkpointed as typed
  ledger facts. The historic `.nodes.yrs`, `<hash>.yrs`, and
  `<hash>.overlay.yrs` files are not compatibility inputs: filesystem sidecars
  are write-only crash-state sidecars driven as effects from typed state. Rename
  migration rekeys typed events and never reads or moves these files.

## Recursive reconciliation — `reconcile_component` (`#qnodemerge3`)

The Phase 1/2 layers stop at the top-level node (component + interstitial) and run the leaf
text `merge` on each *whole* component. Phase 3 drills the **same** keyed reconciliation one
level deeper, inside any component whose body is a sequence of keyed children:

- **List components** (`queue`/`backlog`/`review`/`done`): each `- …` markdown item is a child,
  keyed by its durable `#id` (the identity the strike paths already use) or, for a free-text item,
  its normalized text (strike markers / pin glyphs / checkbox stripped, so a strike keys the same).
  Continuation lines attach to the preceding item; leading text is a reserved preamble child.
- **`exchange`**: each `### Re:` block is a child keyed by its heading (minus the working-tree-only
  ` (HEAD)`). A prompt typed into the exchange attaches to the block it falls within, so it
  reconciles within that block and never cross-splices into another.

`reconcile_component_body` matches `ours`/`theirs`/`base` children by key (React-VDOM style):

- **Matched, different** → leaf `merge` against *that child's own* base. Two edits to different
  keyed children land in separate sub-trees and can never contend — editing queue item B while
  item A is the running head leaves A byte-identical, and an interleaved exchange prompt converges
  alongside a freshly appended `### Re:` block with zero cross-block splice.
- **Key on one side only** → a pure **insert** (key absent from base — kept, so a concurrent user
  queue addition is never dropped) or a **delete** (key in base, the other side unchanged — honored
  for list items; **never** for committed `exchange` blocks, the per-block `#ipc-crdt-response-drift`
  guard). A modify-vs-delete conflict keeps the surviving content.
- Order is `order_union`: ours' order is the spine, theirs-only inserts woven in after their nearest
  placed theirs-predecessor — deterministic for the common append/insert-on-one-side cases.

Segmentation is lossless (`concat(children) == body`) and the whole path is **fail-safe**: an
unsplittable component, malformed marker framing, ambiguous (duplicate) keys, or any leaf merge
error falls the component back to the flat whole-component `merge` (current behavior), so Phase 3
strictly narrows contention without widening the corruption surface. Because the engine lives in
the shared `merge_aligned_nodes`, both `merge_by_component` (whole-doc base) and
`MultiNodeState::merge` (per-node base) get the recursion.

## Op-capture / evented reflection — `merge_with_editor_ops` (`#qnodemerge4`)

Every layer above still reconstructs the `theirs` (editor) side of a merge from a Myers text
diff (`compute_edit_ops`). A Myers diff returns *a* minimal edit script — not necessarily the
edit the user actually performed — so two same-region edits can be mis-attributed and duplicate
(the `#hap7`/`#qdup` corruption family). Phase 4 removes that guess for the editor side by
replaying the editor's **real** operations.

- **`EditorOp`** (`crdt.rs`): an absolute-offset editor mutation in **byte** units —
  `Insert { offset, text }` / `Delete { offset, len }` — matching the editor's
  `DocumentListener.documentChanged` / `onDidChangeTextDocument` events. A replacement is captured
  as a `Delete` then an `Insert` at the same offset. Ops are recorded and replayed in editor order,
  each offset absolute against the buffer *after* all prior ops. `serde`-serializable for the
  capture sidecar.
- **`replay_editor_ops(base, ops)`**: reconstructs the editor's final text by applying the ops in
  sequence, returning `None` on any out-of-bounds or non-char-boundary offset.
- **`merge_with_editor_ops(base_state, ours, theirs, theirs_ops)`**: same contract as `merge`, but
  when `theirs_ops` is supplied it feeds the editor's exact ops into the `theirs` CRDT side
  (`apply_editor_ops`) instead of the diff-guess (`apply_ops`).

**Safety gate (the acceptance invariant "ops replay equals editor-observed state").** The captured
ops are trusted only when `replay_editor_ops(base_text, ops) == theirs_text` against the *resolved*
merge base (after stale-base / shared-prefix advancement). If the ops were captured against a
divergent base (advanced base, a missed event), replay won't match and the merge transparently
falls back to the diff-guess — never worse than today. A conservative offset-safety guard also
restricts op-replay to ASCII `base`/`theirs` (Yrs index semantics for non-ASCII are not asserted
here); per-component merge means a unicode glyph in `queue` never disables op-replay for ASCII
`exchange` prose. `merge` delegates to `merge_with_editor_ops(.., None)`, so the existing path is
byte-identical until ops are supplied.

**Status.** The op-capture consumer and supply side are shipped: a per-document op-capture sidecar
(record/load/clear/GC), FFI ingestion (`agent_doc_record_editor_op`), live `merge_contents_crdt`
consume+clear wiring, and thin JetBrains `DocumentListener` / VS Code `onDidChangeTextDocument`
reporters. Realtime editor-to-editor convergence now delivers the merged result with node-keyed IPC
patches (plus legacy component fallback), so peer buffers can apply targeted node inserts/replaces
under the same ACK proof used by CP-to-editor writes. The zero-duplication live eyeball (a
concurrent live edit + agent write) remains `[operator-verify]`.

## Roadmap — recursive AST-node merge

`merge_by_component` is the component-level (coarsest) rung. The full model applies node
isolation to the **entire document AST**, reconciled by durable node identity the way a React
Virtual DOM keys its children.

| Phase | What it adds |
| --- | --- |
| `#qnodemerge1` ✅ shipped | Component-scoped merge (`merge_by_component`) — the anti-corruption rung above. |
| `#qnodemerge2` ✅ shipped | Per-node CRDT **state persistence** — `MultiNodeState` (`crdt.rs`) persists one independent Yrs state per top-level node into a single structured container (`<hash>.nodes.yrs`), the per-component successor to the whole-doc `<hash>.yrs`. Each node carries its own stable base across cycles (an untouched node re-encodes byte-identically; a changed node's base advances on its own). Migrates a legacy whole-doc `<hash>.yrs` lazily, rebuilds (GCs) per node every save/compaction, and follows the document across renames. See *Durable per-node base* below. |
| `#qnodemerge3` ✅ shipped | **Recursive AST-node reconciliation** — `reconcile_component` (`crdt.rs`) drills the keyed reconciliation *inside* each component via the shared `merge_aligned_nodes` per-node path. Queue/backlog/review/done items are keyed by their durable `#id` (or normalized text); `### Re:` blocks are keyed by heading. Children are matched by key (React-VDOM style): a matched-but-different child runs the leaf text `merge` against *its own* base child, a key-on-one-side child is a pure insert (kept) or delete (honored only on a clean delete-vs-unchanged; never for committed `exchange` blocks). The whole-component text `merge` stays the leaf and the fallback (unsplittable component, malformed framing, or ambiguous/duplicate keys). See *Recursive reconciliation* below. |
| `#qnodemerge4` 🟡 core shipped | **Op-capture / evented reflection** (highest-leverage accuracy lever) — feed real editor operations (`DocumentListener.documentChanged` / `onDidChangeTextDocument`) into the per-node model instead of reconstructing edits from a text diff, removing the diff-guess. The merge **consumer** is shipped: `EditorOp` + `replay_editor_ops` + `merge_with_editor_ops` (`crdt.rs`) replay the editor's exact ops into the `theirs` CRDT side behind a replay-exactness safety gate. Remaining (`#qnodemerge4wire`): the **supply** side — capture-sidecar persistence, the FFI ingestion entry point, and the thin plugin `DocumentListener` / `onDidChangeTextDocument` reporters that record ops live. See *Op-capture / evented reflection* below. |
| `#qnodemerge5` | **Surface true conflicts, never fabricate** — for a genuine concurrent edit to the *same* leaf node (information-theoretically underdetermined), present both versions for operator resolution instead of silently auto-merging text neither side wrote. |

Accuracy ordering is the dependency order: `2 → 3 → 4`, with `5` sequenceable any time after
`3`. The structural layer (keyed reconciliation) gets the document into the right *shape*; the
accuracy comes from feeding it real ops (`4`) on a correct per-node base (`2`/`3`) and
refusing to guess on genuine conflicts (`5`).
