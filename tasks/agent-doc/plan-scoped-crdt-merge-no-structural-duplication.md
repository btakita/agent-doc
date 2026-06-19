# Plan — #qdup: scope the CRDT/semantic merge to the structural part it applies to

## Problem (two operator-reported instances)

agent-doc's CRDT/semantic merge **duplicates operator input** when the operator
edits the document (typically a queue prompt) while a turn is running:

1. **virtuousai.md** (operator): "As I was typing in a queue item into virtuousai.md,
   parts of the document was duplicated, corrupting the document."
2. **equityfundingsource.md** (operator, 2026-06-19): "My equityfundingsource.md queue
   prompt `:pushpin: All upload preview dialogs should be full screen. deploy` was
   duplicated by agent-doc."

The merge currently operates over a scope wider than the structural part actually
being changed, so a concurrent operator edit to one structural region (e.g. a single
`agent:queue` item) gets re-applied/duplicated against the whole component or
document.

## Operator-specified fix direction

(From the virtuousai report — the operator already prescribed the design.)

1. **Constrain the merge to the structural part it applies to.** Do not merge at the
   whole-document or whole-component granularity when the change is to one structural
   node. Run **multiple smaller merges, one per structural part**, as needed.
   - This removes the possibility of structural corruption and reduces conflicts:
     a concurrent edit to queue-item A cannot duplicate/corrupt queue-item B or the
     surrounding structure.
2. **Deleted-structure rule.** If the operator deletes part of the structure in the
   document and there is new agent content targeting that deleted structure, **mention
   the fact in the `agent:exchange`** but do NOT attempt to merge it back — the
   in-editor document is the source of truth.

## Implementation sketch (to validate against the CRDT/merge code)

- Identify the structural unit of merge: component → item node (`agent_doc_markdown_ast`
  node_key) rather than raw text spans / whole-component bodies.
- Drive merges per changed node: compute the changed/added/removed nodes (the
  `semantic_diff` node_events already exist) and apply the CRDT merge **per node**,
  not over the joined component body, so an operator insert into one queue item cannot
  reflow or duplicate adjacent nodes.
- Deleted-structure detection: when an agent patch targets a node the operator removed
  this cycle, route a note into `agent:exchange` instead of resurrecting the node
  (compose with the existing `#semmerge` per-structure constraint and the
  content_ours / live_prompt_drift adoption path).
- This composes with `#fullboundary` (fewer concurrent writers ⇒ fewer merges) and
  `#semmerge` (per-structure constraint) — `#qdup` tightens the *granularity* of each
  merge so a surviving concurrent edit cannot duplicate structure.

## Tests

- SimWorld: operator inserts a queue item while an agent exchange patch is in flight →
  the queue item appears exactly once; no adjacent node is duplicated.
- SimWorld: operator deletes a structural node that an agent patch targets → a note
  lands in `agent:exchange`, the node is NOT resurrected, no duplication.
- Unit: per-node merge of a component with one changed node leaves sibling nodes byte-identical.

## Notes

Touches the CRDT/merge core (`crdt.rs` / `merge.rs` / the converge path) — the same
machinery currently producing the live `content_ours` / `live_prompt_drift` adoption
this session. Implement in a focused cycle with `make check` (+ `make tmux-ci` if the
converge/live-editor path is touched). Do NOT rush a merge-core change in a session
that is itself actively drifting.
