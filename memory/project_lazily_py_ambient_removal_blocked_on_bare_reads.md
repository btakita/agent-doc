---
name: project_lazily_py_ambient_removal_blocked_on_bare_reads
description: "RESOLVED 2026-07-22 (lazily-py 0.35.0): full ambient slot_stack removal SHIPPED. The blocker was BARE reads (not multi-context) — fixed by threading the caller's Compute reader, ctx-param family factory. Value-threading now sole tracking surface across all 8 bindings."
metadata:
  node_type: memory
  type: project
  originSessionId: 319d60e9-f1fe-4008-a70c-af4e66292817
  modified: 2026-07-22T06:36:04.326Z
---

**RESOLVED — full ambient `slot_stack` removal SHIPPED in lazily-py 0.35.0
(2026-07-22).** py was the last binding on the ambient bridge; all 8 now use
value-threading as the sole tracking surface.

**The diagnosis that unblocked it** (answered "can StateMachine/relay just hold a
ctx?"): NO — and they already do. A `Compute` has two parts: the store dict (the
held ctx supplies it) and the **reader** = the currently-recomputing node, which
every edge attributes to (`_subscribe_reader(dep, self.node)`; the store plays no
part in *who* subscribes). One machine/relay is read by MANY readers over its life
(each created after the instance), so there is no single reader to bake in at
construction — `RelayCell` already held+threaded its ctx yet still needed
`slot_stack`, the clean disproof. StateMachine/RelayCell are single-ctx-bound, NOT
inherently multi-context; the missing datum was the reader, not the ctx.

**The fix (migration A, ctx-param):** thread the caller's `Compute` view into each
bare-read site. Feature reader methods gained an optional `ctx` (`is_full(ctx)`,
`state_at(ctx)`, `output(ctx)` → `ctx.read(node)`); properties/dunders that can't
take an arg (`StateMachine.state`, `len(cm)`) kept an untracked snapshot + a
ctx-taking companion. Family factory type changed `Callable[[K],V]` →
`Callable[[C,K],V]` (~40 call sites; `lambda k:` → `lambda _c, k:`; tracking ones
`lambda c, k: c.read(src)+k`). `lookup` dual-use (factory AND direct value
producer) resolved by keeping it 1-arg and adapting only at family call sites. Then
the bridge was deleted entirely (all 3 read consumers, push/pop, gate scaffolding).
521 tests green unconditionally; `make check` exit 0. See
[[project_lazily_fortified_compute_wrapper_spike]].
