---
name: reference-lazily-hybrid-staleness-cascade-defect
description: Eager cone marking and lazy pull are each correct for reactive staleness; combining one-level marking with a cache-trusting read loses writes at depth 2 — shipped in lazily-dart and lazily-go async contexts
metadata: 
  node_type: memory
  type: reference
  originSessionId: 9d435e15-6b59-41e8-9d3e-16220a325346
---

**The defect:** a chain `cell -> a -> b` where writing the cell invalidates `a`
but leaves `b` serving a stale cached value **forever**. Found and fixed
2026-07-19 in `lazily-dart` (`c91a32a`) and `lazily-go` (`bdfdbce`).

**Why it happens.** Two strategies for making a write observable at transitive
depth are each *correct*:

| Strategy | Write does | Read does |
|---|---|---|
| Eager marking | walks the whole dependent cone, marks all dirty | trusts the flag |
| Lazy pull | marks little or nothing | refreshes dependencies recursively before deciding to recompute |

The spec describes lazy pull ("On `get`, a slot first refreshes its own
dependencies (recursively, lazy pull)"). **The unsound combination is the
hybrid**: mark one level eagerly *and* let reads short-circuit on a clean node.
Each half is defensible alone; together the mark never arrives and the read never
looks past it. **No single line of code is wrong**, which is why it survives
review.

Both bindings hit it the same way: an async slot tracked staleness with a
revision counter plus a `Resolved`/`.resolved` state whose read path returned the
cache directly, while the invalidation handler notified only the written cell's
immediate dependents.

**How to check a binding:** don't grep for a non-cascading handler — that is
correct under pure lazy pull. Determine (1) which strategy it uses, and (2)
whether the read fast path short-circuits. The bug exists only where a *bounded*
mark meets a short-circuiting read. Sync contexts in both bindings were fine.

**Fix shape:** walk the full cone with an explicit stack in the context (not
recursion from the node — keeps the batch gate at one entry point). Terminate by
**edge consumption** (delete the dependent set on visit) rather than a visited
set; diamonds and cycles then die naturally, and edges re-register on recompute.
Collect effects during the walk and schedule them **after** — scheduling inline
mutates the dependent map the walk is still reading. Effects are frontier leaves.

**Companion defect, dart-only:** its async effect never detached prior
dependencies on rerun, so once invalidation consumed the edge sets,
re-registration was skipped and the effect went **permanently deaf after exactly
one rerun**. Test effects across TWO successive writes; one rerun cannot see it.
go's `runEffect` already detached, so it was unaffected.

**Formalized:** `hybrid_serves_stale_value_at_depth_two` in
`lazily-formal/LazilyFormal/Reactive.lean` (`e6c4cd5`), three nodes,
`decide`-checked. Spec fixture:
`conformance/reactive-graph/transitive_invalidation_reaches_depth.json` — depth-4,
asserts observable values only (both strategies conform), and requires replay
against **every context a binding ships**.

**The systemic finding underneath it:** a depth-2 fixture encoding this property
already existed (`dispose_detaches_edges_both_directions.json`) and **only
`lazily-rs` replays the corpus**. Neither binding's own tests could have caught
the bug — every async slot test in both repos reads a cell directly and nothing
composed a slot on a slot. Found only by reading one implementation beside
another. See [[reference-lazily-conformance-delivery-three-failure-modes]].
