---
name: reference_lazily_go_thread_safe_map_data_race_ci_red
description: FIXED in lazily-go v0.23.2 — every ThreadSafeReactiveMap READ ran off the context lock while every write took it; a Get is a graph mutation in this kernel
metadata: 
  node_type: memory
  type: reference
  originSessionId: a5a9e782-82c8-4457-962f-9adae81c9c8a
  modified: 2026-07-26T19:11:14.632Z
---

**Found and fixed 2026-07-26 (v0.23.2).** lazily-go CI had been red for 10+
consecutive runs; `TestTSComputedMapConcurrentMaterializationIsConfluent` failed
under `-race` with ~14 `WARNING: DATA RACE`. **v0.23.0 and v0.23.1 were both cut
on red.** CI is green again on `289a19e`.

**The bug, and the reason it is easy to write:** `ThreadSafeReactiveMap` took
`tsctx.WithLock` for mint, the membership/order signal bumps, entry disposal, and
`Set` — every *write*. Every *read* went straight to the graph. That reads as
correct until you notice **a read is not passive in this kernel**: `Get` on a
stale derived entry runs `refresh` → `recomputeNow` → `Context.newCompute`, which
increments `computeGen` and `cachedCount` on the shared single-threaded
`Context`. Two goroutines reading two **different** keys write the same fields.
A `Source` read mutates the dependency edge set too when the read surface is a
`*Compute`.

Fix: `GetOrInsertWith`, `Observe`, `Keys`, `Len`, `ContainsKey` route through
`read`/`track` helpers that take the lock. It is reentrant, so an entry's compute
reading back into the map is fine, and `mu` is still never held across graph work
so lock order stays context-then-`mu`.

**Three ways the race-detector mutation check failed to discriminate** — all
worth reusing, because `-race` probes are probabilistic and a green run proves
nothing on its own:

1. The tracked-read soak's mutator called `MoveTo(k, k)` — a no-op on an
   insertion-ordered map. The order signal never moved, so unlocking the `Keys`
   read raced with nothing.
2. `Observe`'s lock is load-bearing only when an entry node is concurrently
   **written**, and a *computed* map never dirties one (entries compute once and
   keep their value). A **source** map with a `Set` mutator is the shape that
   exposes it.
3. Readers and the writer must contend on the **same** entry. Spread over 20 keys
   the detector missed it; concentrated on 4 it was reliable. Run the sweep with
   `-count=3` and repeat it — verdicts at `races=1` are marginal.

`make check` is green even when `make race` is red — judge this surface by
`make race`. Related: [[reference_lazily_queue_family_release_2026_07_26]],
[[feedback_conformance_tests_drive_real_behavior_not_runner_bookkeeping]].
