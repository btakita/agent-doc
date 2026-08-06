---
name: project_lazily_gdscript_binding_direction
description: "GDScript is the chosen shape for a Godot lazily binding (10th column), not a GDExtension over lazily-cpp; WeakRef back-edges accepted"
metadata: 
  node_type: memory
  type: project
  originSessionId: 94dfa3d2-6288-46f3-9c9e-a497138b7902
  modified: 2026-07-30T04:13:10.038Z
---

Decided 2026-07-30 while scoping a Godot SDK for Haiven work (see `src/haiven-dev`):
a **pure GDScript** lazily binding is preferred over a GDExtension wrapper around
lazily-cpp.

**Why not GDExtension**, despite lazily-cpp's reactive core being wasm-clean
([[reference_lazily_cpp_wasm_clean_header_subset]]): the blocker is **consoles**, not
web. Switch/PS/Xbox Godot builds come from third-party porting houses, so you cannot
produce those GDExtension binaries yourself. GDScript ships wherever Godot ships with
no binary matrix, no godot-cpp pinning, no per-platform debug/release pairs tracking
Godot minor releases. Design argument points the same way: the wasm-hostile lazily-cpp
pieces (`transport.hpp`, `reliable_sync.hpp`) are exactly what a Godot SDK should
replace with Godot's own `WebSocketPeer` anyway, so what you actually want from lazily
is the layer with zero native deps.

**The one novel engineering problem: `RefCounted` has no cycle collector.** No other
binding faces this (rs/cpp use `Weak`/`weak_ptr`; py/js/dart/kt/cs/go have GC).
**Operator accepted WeakRef back-edges** (`source → dependents` weak, forward
`Computed → deps` strong), which implies:

1. **`ctx.scope()` becomes the sole strong owner.** An `Effect` with only a weak
   inbound edge is collectable as soon as you stop holding it, unlike the GC bindings.
   The reverse-creation-order member list IS the ownership record. An effect created
   outside any scope should be an error, not a silently-collected no-op.
2. **Prune dead `WeakRef`s before any edge-count read.** GDScript has no finalizer
   hook, so pruning must be amortized into propagate/attach or the O(1) dedup counts
   read high.
3. **Key the edge index on `get_instance_id()`** — monotonic in Godot, so ids never
   alias. Orthogonal to lazily's own `recycled_id_inherits_nothing` semantics.

Fixtures that will catch a wrong weak/strong split:
`teardown_runs_members_in_reverse_creation_order`, `disposal_does_not_run_surviving_effects`,
`cross_scope_teardown_hazard`, `dispose_detaches_edges_both_directions`,
`scope_teardown_equals_fold_of_disposals`, `disarm_disposes_nothing`.

**Entry gate:** `lazily-spec/coverage.json` = 9 languages / 64 rows over 134 fixtures
in `lazily-spec/conformance/`. Follow the lazily-cs staged pattern
([[project_lazily_cs_binding]]): rows 0+15 first, empty ledger, corpus replays via
`JSON.parse_string` + `FileAccess`.

Optional later: desktop-only GDExtension fast path over narrow `cell.hpp` +
`collections.hpp` includes, gated on real profiling. Safe to maintain alongside
because the fixture corpus is what prevents divergence.

Related: [[reference_lazily_no_observers_only_effects]],
[[reference_lazily_edge_index_teardown_scope]],
[[reference_lazily_edge_set_vs_observer_registry_test]]
