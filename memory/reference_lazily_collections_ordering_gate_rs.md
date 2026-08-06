---
name: reference_lazily_collections_ordering_gate_rs
description: The collections ordering fixtures had a dead assertion in lazily-rs and no coverage at all on 2 of 3 flavors — the gate shape that fixes it
metadata: 
  node_type: memory
  type: reference
  originSessionId: a7a97987-070c-4a20-bdef-034405380ac8
  modified: 2026-07-26T05:15:04.831Z
---

Two independent ways the keyed-collection ordering contract reported green while
testing nothing in lazily-rs (fixed 2026-07-26, commit `325ee7d`):

1. **Dead assertion.** `tests/collections_conformance.rs` read
   `step.get("invalidates")`, but the fixtures nest `invalidates` under
   `expected`. It was always `None`, so `assert_invalidation` — the entire point
   of `cellmap_independence.json` — never ran. **Check this in every binding.**
2. **One flavor of three.** Only `SourceMap` was replayed; `ThreadSafeSourceMap`
   and `AsyncSourceMap` had no `move_*` at all and no fixture asked. But
   `coverage.json` marks the ordering rows green for **every binding**.

The gate: `tests/collections_family_conformance.rs` — one replay engine over a
`MapModel` trait, three impls, with the **only** async-coloured obligation
isolated to `settle`. Same shape as `tests/reactive_graph/` and lazily-zig's
`Engine(comptime Model)`.

Two things it must do or it is theatre:
- **Assert readers actually primed** before measuring invalidation, else "stayed
  cached" is trivially false and "was invalidated" trivially true.
- **Mutation-check per flavor.** Three that must each turn the new rows red:
  routing a reorder through the membership bump; making a move a remove +
  re-mint; leaking a value write into the order plane.

`AsyncContext` needed `is_set` and the async handles needed `PartialEq`/`Debug`
before `handle_stable` was even expressible. The async map's `observe` is **not**
generic over the read surface (unlike `keys`/`len`/`contains_key`), so a tracked
per-entry read has to go through `handle()` + `actx.get()`.

[[project_lazily_reactivemap_unification_rs_reference]]
[[feedback_conformance_tests_drive_real_behavior_not_runner_bookkeeping]]
[[reference_lazily_conformance_delivery_three_failure_modes]]
