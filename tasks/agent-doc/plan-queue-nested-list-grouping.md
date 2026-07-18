# Plan: nested queue lists, `ol`/`ul` semantics, and DAG reconciliation

Status: DESIGN — not implemented. `#queuenest`

## The decision that unlocks the rest

**`ol` is sugar that compiles to `after=` edges. `after=` stays the single
source of truth for dependencies.**

Everything else follows from this, including the reordering question that was
hard to answer directly.

### Why not make `ol` authoritative

`after=#id` already encodes the DAG, and a priority-weighted topological sort
already consumes it (`#queue-auto-dag-priority`). Making list *shape* a second
authoritative encoding of the same fact creates two sources of truth that can
disagree.

That exact shape has been the bug surface all session:

- legacy `queue_active:` vs canonical `queue:` — `#qstartinert`, three separate
  fixes because activation, drainability, and convergence each trusted a
  different one.
- queue-maintenance persistence vs tracked-work persistence — `#queueatcreate`,
  where one write grew the backlog and silently dropped the queue head.

A third instance is predictable, not hypothetical.

### Why the reorder question was hard

A DAG is a partial order; an `ol` is a total order. Any `ol` is therefore just
*one valid linearization* of the DAG. So "the operator reordered the `ol`" is
ambiguous on its face — they may have chosen a different valid linearization
(fine) or violated a real edge (not fine). Position alone cannot distinguish
these. Checking against `after=` can.

Once `ol` compiles to edges, the ambiguity disappears: reordering an `ol`
re-derives its edges. There is nothing to reject.

## Recommended semantics

| Shape | Meaning |
|---|---|
| top-level `ul` | the queue itself — unordered, position is preference not constraint |
| nested `ul` | a group of *related* items, no ordering constraint between them |
| nested `ol` | siblings that ARE sequential — compiles to `after=` chaining each to its predecessor |
| parent item | descriptive only: the epic/group label. **Never executable.** |
| leaf item | the executable unit — only leaves become `do [#id]` heads |

Top level stays `ul` and nests from there, as proposed.

## On rejecting or reverting an operator edit

**Do not.** It contradicts the invariant this codebase already enforces:
operator-visible document text is authoritative. Reverting the operator's own
reordering is the same class of move as the `content_ours` clobber the write
path is explicitly built to refuse.

Not needed either — correctness does not depend on queue order. The drain
already refuses to run a dependent before its prerequisite (`after=` gating in
`head_is_drainable` / the auto-DAG sort). A "wrong" order is a display
inconsistency, not a correctness failure.

So: accept the edit, re-derive edges, and let the sort express the constraint.

The "generate a nested list with dependencies ordered first" idea fits here —
but as a **derived view or an explicit command** (`agent-doc queue regroup`),
never an automatic rewrite of text the operator just typed.

## Implementation sketch

1. Indent-aware `document_queue::parse` — currently flat. Retain depth + marker
   kind (`-` vs `1.`) per entry.
2. Leaf-only execution: parents never yield `do [#id]` heads.
3. `ol` -> `after=` compilation at ingress, next to `#queueingressorder`.
4. Render must stay **insert-only / byte-preserving**. `#queueatcreate` already
   hit this: a parse -> render round-trip silently rewrote
   `- /goal complete the entire queue` to `/goal ...`, dropping the operator's
   list marker. Nesting makes round-trips more tempting and more damaging.
5. Manual sibling reordering: plain text edit, no renumbering convention needed
   if `ol` markers are all `1.` (standard markdown auto-numbers on render), which
   also avoids churn when items are inserted.

## Open

- Do groups need ids (`[#epic]`) for `after=` to reference a whole group?
- Should `priority=` on a parent apply to all its leaves?

## Related

`#queueingressorder` (shipped), `#pinoperatoronly` (shipped), `#queueatcreate`
(shipped), `#qauthorder`, `#queue-auto-dag-priority`.
