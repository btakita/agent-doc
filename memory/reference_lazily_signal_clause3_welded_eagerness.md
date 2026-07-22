---
name: reference_lazily_signal_clause3_welded_eagerness
description: Signal clause 3 (one compute per batch) caught 4 lazily bindings via 2 mechanisms; go blocked on a consuming cascade
metadata: 
  node_type: memory
  type: reference
  originSessionId: b2d735fa-2cef-4742-a242-e863cca210c7
---

`#lzsignaleager` (2026-07-20): four normative Signal clauses + 3 fixtures in
lazily-spec, 12 Lean theorems in `LazilyFormal/Signal.lean`. **Clause 3** — a
batch of N writes produces exactly ONE compute, at batch exit — caught **four
bindings across two unrelated mechanisms**. All four produced *correct values*
while doing 2–3× the computes, which is why the other 11 reactive-graph fixtures
never saw them.

- **Mechanism A, welded eagerness (py, dart, go):** a signal-specific slot
  subclass whose invalidation handler re-pulls inline. Re-pulling during
  invalidation is earlier than the effect flush, so compute count scales with
  changed sources. No effect anywhere in the construction. Three bindings reached
  the same wrong design independently, in three languages.
- **Mechanism B, a batch that does not bound the flush (zig ThreadSafeContext):**
  `setCell` flushed unconditionally while `batch` only nested a depth counter.
  **A composition is not sufficient if the batch boundary does not gate the
  flush.**

Passing bindings (rs, cpp, js, kt) all build Signal as memo-slot + puller-Effect;
clause 3 falls out free because effects are scheduled, not inline.

**lazily-go is blocked deeper** — see `tasks/software/lazily-signal-followups.md`.
Its cascade **consumes reverse edges as it walks** (`reactiveBase.invalidate`
core.go:116, `AsyncContext.propagate` async_context.go:618). Marking a dependent
clean without recomputing means it never re-runs `track()`, never re-registers,
and the next change is lost at depth two = `hybrid_serves_stale_value_at_depth_two`.
So the memo guard cannot move to pull time until the cascade is non-consuming.
`Effect.relinkDependencies` exists *only* to repair edges the cascade ate.
Decision 2026-07-20: keep composed Signal, **ledger the divergence, do not
migrate**. Reject the flush-time-unmark shortcut — it's a third hybrid variant.

**The async surface is the family's weakest, systematically:** cpp async = stub
with no dependency graph and no signal API; zig async = no lazy mode at all
(every derived slot eager); py/dart async = no signal constructor.

New observable: **`computes_of`** (cumulative compute invocations per node).
Required because an eager signal and a lazy memo return identical values for
every read sequence — a values-only corpus cannot tell `signal()` from `memo()`.

[[reference_lazily_hybrid_staleness_cascade_defect]]
[[reference_lazily_conformance_delivery_three_failure_modes]]
