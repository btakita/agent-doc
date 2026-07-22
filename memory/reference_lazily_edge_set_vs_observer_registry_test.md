---
name: reference-lazily-edge-set-vs-observer-registry-test
description: "To tell a lazily dependency-edge set from an observer registry, apply one mechanical test: anything that survives an invalidation is not a graph edge"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 9d435e15-6b59-41e8-9d3e-16220a325346
---

> **Anything that survives an invalidation is not a graph edge.**

Dependency edges in lazily are re-discovered on every recompute — the tracking
stack clears them and the next run re-registers whatever it actually reads. That
is what makes dependencies *dynamic*. A collection that **persists** across
invalidation is therefore not participating in dependency tracking, whatever it
is named, whatever its docstring claims, and whatever it sits beside.

**Why this needs to be a rule.** On 2026-07-19 `lazily-py` was found to carry
**three** observer registries — on `Cell`, on `Slot`, and on `Signal` — and every
one was either labelled or assumed to be dependency-graph state. Two independent
readers, with the source open, misclassified one each (I called `Signal`'s a
graph edge; the subagent called `Slot`'s an edge set and recommended a spec
carve-out for it). In every case the registry sat next to a real edge set with a
near-identical name — `_subscribers` beside `_parents` — and in every case the
deciding fact was a single line in the same method: the edge set is rebound and
cleared on invalidation, the registry is iterated and kept.

**How to apply:** run the criterion mechanically before reasoning about intent or
reading prose. In py the tell was in `slot.py`'s own comment: *"External
subscribers persist across touches (they are not reactive edges) ... The
auto-discovered parents are reactive edges: rebind-then-clear."* Grep for what
the invalidation path does to each collection, not for what it is called.

Also check subclasses: `Effect` subclasses `Slot` in py and assigned
`_subscribers = None` in two methods, which a Slot-only removal would have
broken.

Generalizes past lazily — it is the same failure mode as trusting a
hand-maintained divergence table instead of running fixtures
([[reference-lazily-conformance-delivery-three-failure-modes]]): believing a
description rather than checking the mechanism.

Spec text: `lazily-spec/docs/reactive-graph.md` § "How to tell an edge set from a
registry" (`87abc85`). Related:
[[reference-lazily-no-observers-only-effects]].
