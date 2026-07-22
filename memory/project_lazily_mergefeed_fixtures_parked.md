---
name: project_lazily_mergefeed_fixtures_parked
description: Five lazily-spec conformance fixtures are parked on branch pending/mergefeed-fixtures until runners implement the merge ops
metadata: 
  node_type: memory
  type: project
  originSessionId: 5fa6f804-041f-4471-ae01-08506e5c78b4
---

As of 2026-07-20, five reactive-graph conformance fixtures live on
`lazily-spec` branch **`pending/mergefeed-fixtures`**, NOT on `main`:
`merge_per_settled_cone_not_per_write`, `merge_folds_synchronously_in_batch`,
`merge_cell_acquires_no_dependency_edge`, `exact_fold_paths_stay_exact`,
`feedback_drain_bound_reports_exhaustion`.

**Why:** they cite `merge_cell` / `merge` ops and `merges_of` /
`drain_exhausted` assertions that no runner implements yet. Two independent
mechanisms make this a flag day:

1. Runners **panic on unknown ops rather than skipping** (`lazily-rs`
   `tests/reactive_graph/engine.rs:336`).
2. Stronger — the runner asserts **the on-disk fixture set equals its hardcoded
   `FIXTURES` list exactly** (`tests/reactive_graph_conformance.rs:38-40,61`;
   `[&str; 14]`). A new file on disk fails the build even if no runner ever
   dispatches its ops.

So merging to `main` turns all eight corpora red simultaneously. Mechanism (2)
also means a runner can never silently ignore a new fixture, which is the
"green suite is an empty suite" failure §9.5 guards against.

**How to apply:** land them only after Track B step 2 — the `merge_cell`/`merge`
ops implemented in all eight runners. The allowlist widening that permits them
is already on `main` (`tests/test_schema_conformance.py`, commit 845283a) and
breaks nothing on its own, because it only permits.

Same runners-accept-before-fixtures-emit ordering the plan uses for op renames.
See [[project_lazily_memo_rename_plan_status]].
