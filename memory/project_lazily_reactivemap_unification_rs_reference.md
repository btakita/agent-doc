---
name: project_lazily_reactivemap_unification_rs_reference
description: lazily-rs Phase A of the ReactiveMap unification is DONE and is the reference shape the other 8 bindings converge on
metadata: 
  node_type: memory
  type: project
  originSessionId: a7a97987-070c-4a20-bdef-034405380ac8
  modified: 2026-07-26T05:14:51.468Z
---

Plan: `tasks/plan-lazily-reactivemap-unification.md`. **lazily-rs Phase A landed
2026-07-26** (commit `325ee7d`) and is the reference implementation for the
remaining 8 bindings.

The shape:

- **`src/keyed_order.rs` — `KeyedOrder<K, H>`**: present set + authoritative key
  order + `position` + the move algebra. Closure-free, context-free, **no
  interior mutability** — each flavor wraps it in its own `RefCell`/`Mutex`.
- **Reactivity deliberately stays OUT of the core.** Mutators return
  `Mutation::{Changed,Unchanged}` / `Move::{Absent,Unchanged,Reordered}`; the
  shell decides which signal cells to bump. A core that owned reactivity would
  have to fake it with polled counters — which is exactly lazily-zig's
  non-reactive map.
- **Do NOT route through `SyncReactiveGraph`.** Its `computed` imposes
  `Send + Sync` on values/closures the concrete single-threaded API does not; an
  `Rc`-capturing caller stops compiling and an impl cannot weaken trait bounds.

Also landed: thread-safe + async gained `position`/`move_*`/`remove`; the
factory stops discarding its tracking view in all three flavors (async's trait
compute widened from nullary `Arc<dyn Fn() -> V>` to
`AsyncEntryCompute<V> = Arc<dyn Fn(&AsyncComputeContext) -> V + Send + Sync>`);
new `AsyncContext::eval_detached` + `is_set`; `PartialEq`/`Eq`/`Debug` on
`AsyncSource`/`AsyncComputed`.

**The plan's "remove is handle-trait surgery" blocker was wrong** — both
`ThreadSafeContext` and `AsyncContext` already expose
`dispose_cell`/`dispose_slot`, which is stronger than the sync
`clear_dependents` and observably the same.

Remaining: sequencing steps 1–5 for the other 8 bindings, plus the surveyed
per-binding defects. See [[reference_lazily_collections_ordering_gate_rs]] for
the test harness shape.
