---
name: reference-lazily-no-observers-only-effects
description: "lazily bans observer APIs on every reactive (Cell/Slot/Signal) as of 2026-07-19 — observation is a declared graph edge, callbacks are Effects, streams are Topic"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 9d435e15-6b59-41e8-9d3e-16220a325346
---

**No lazily reactive exposes an observer API** — not `Cell`, not `Slot`, not
`Signal`, not `MergeCell`. Mandated in `lazily-spec/docs/reactive-graph.md`
§ "Reactives have no observers" (`#lzdartobservercow`, 2026-07-19). A binding
conforms **by not having one**. `Topic`/`Queue` `subscribe` is retained and is the
only legitimate `subscribe` in the family.

**Removed 2026-07-19:** py `f2ff1f0` (three registries — Cell, Slot, Signal),
dart `e741843`, go `f7ba64c`, zig `c796e43`. rs/cpp/js/kt never had one.
`on_transition` is now an `Effect` everywhere (reference form:
`lazily-rs/src/state_machine.rs`).

**Why** — the mechanism accumulated six normative clauses as four bindings
disagreed, and six clauses in they still disagreed while the spec had been wrong
twice in one day. The sixth ("delivery is per write, `batch` does not coalesce
it") contradicted the batching model every other primitive honours. A primitive
needing six clauses to be safe, still diverging, whose final clause contradicts
its surrounding model, is misdesigned rather than under-specified.

Two costs made it a ban rather than a seventh clause:

- **Memory carried by the graph.** Removing it moved zig's
  `@sizeOf(Cell(u64))` **168 → 32 bytes** (measured, not estimated) — 136B off
  *every cell in every program*, including reentrancy counters and a registration
  counter that existed only to make the notify loop safe.
- **Observers sit upstream of all five coalescence mechanisms and outside all of
  them**: `==` store-guard, memo guard, `batch`, store-without-cascade, and the
  merge fold `⊕`. So an observer cannot distinguish a grouped update from
  separate ones, cannot see what was suppressed, and under a merge policy
  observes *flush timing rather than data* — associativity licenses variable
  flush points, so intermediate values are non-deterministic **by design**. Under
  LWW (`KeepLatest` is `old ⊕ op = op`, and `Cell ≡ MergeCell<KeepLatest>`) a
  losing write is dropped outright, so an observer that fired on it reported an
  event no replica will ever agree occurred.

**The one-line form:** observers report *writes*; the system's semantics are
about *values that survive*.

**How to apply:** if asked to add change-notification to a reactive, use an
`Effect` (it captures `prev` in its closure for `(old, new)` handlers). If the
caller genuinely needs every transition rather than the settled value, that is a
`Topic`, not a callback on a cell — the original design error was a Topic hiding
inside a Cell. Under `batch`, an effect reports `A→C` for `A→B→C`; that is
intended, because a batch asserts atomicity.

Answers the standing objection "isn't a reactive just an observer with extra
steps?" — while `Cell.subscribe` existed it was locally, literally true.

Related: [[reference-lazily-edge-set-vs-observer-registry-test]],
[[project-lazily-realtime-distributed-primitives]],
[[reference-lazily-release-targets]].
