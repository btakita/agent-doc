---
name: project_lazily_cs_binding
description: "lazily-cs is the 9th lazily binding (created 2026-07-25) — reactive graph + merge algebra shipped, 10 phases queued for the rest"
metadata: 
  node_type: memory
  type: project
  originSessionId: 24f78fda-2db5-41b6-8eb1-52772d913df8
  modified: 2026-07-25T22:11:03.872Z
---

`lazily-hub/lazily-cs` (created 2026-07-25) is the ninth binding, submodule at `src/lazily-cs`.
C#/.NET, targets `net10.0`, xunit, `make check`, CI on GitHub Actions.

**Shipped:** coverage rows 0 (reactive graph) and 15 (merge algebra). The whole `reactive-graph`
conformance corpus replays with an **empty ledger** — 20 fixtures, 215 assertions — including the
five merge-feed fixtures and `feedback_drain_bound_reports_exhaustion` that lazily-go parks as
unsupported. Reachable because a `Source<T>` carries its `MergePolicy` in a field (so `merge_cell`
needs no new node kind) and because `Context.DrainBudget` / `LastDrainExhaustion` implement the
bounded feedback drain.

**Deliberate C# divergences** (recorded in the repo's `AGENTS.md`): no `comparable` bound — the
write guard takes an `IEqualityComparer<T>`, defaulting to `EqualityComparer<T>.Default`; node
constructors are extension methods on `Context` (in `Reactive`) so factory names can match the
family vocabulary without colliding with the type names they return; the compute-view
fortification guard is a runtime `StaleComputeException` since C# has no lifetime to bind the view.

**Remaining work:** 46 rows across 10 phases, `#lzcsconcurrency` first (context kinds are a
prerequisite for every later plane, and the multi-model replay it installs is what stops later
phases passing on the sync path while broken on async). Plan:
`tasks/software/lazily-cs-parity-plan.md`.

lazily-formal needs no change — lazily-cs conforms through the corpus, like every other binding.

See [[feedback_conformance_tests_drive_real_behavior_not_runner_bookkeeping]] and
[[reference_lazily_release_targets]].
