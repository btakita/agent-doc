# Plan: dependency reactive availability

## Problem

A Computed can be created before one of its dependency reactives has been
materialized. A non-minting map lookup then returns no handle, so the Computed
cannot register an edge and the dependency's first publication may be missed.

Subscribing to whole-map membership avoids the missed transition, but it is too
coarse: adding any key invalidates every Computed that is waiting for any absent
key.

## Contract

Late-bound dependencies must have stable reactive identity from the first
observation. Availability is data on that dependency reactive, not a retry
condition or a map-wide event:

```text
DependencyReactive<T> = Unavailable | Available(T)
```

A Computed that observes `Unavailable` is subscribed to the exact dependency
reactive. Publishing `Available(value)` invalidates only consumers of that
reactive. No sleep, retry loop, filesystem probe, SQLite read, or global
membership dependency participates in the transition.

## Design

1. Add a per-key dependency-reactive primitive to the single-threaded,
   thread-safe, and async keyed families.
2. Materialize the dependency reactive with `Unavailable` on first observation;
   do not materialize the expensive value or derived computation.
3. Make first publication a normal Source transition to `Available(value)`.
4. Preserve stable identity across later value changes. On logical removal,
   transition back to `Unavailable`; dispose the slot only when its owning scope
   is torn down.
5. Keep existing non-minting `observe` behavior for callers that intentionally
   do not want a dependency edge. Give the new operation a descriptive name
   such as `observe_dependency`.
6. Define equivalent wire/state-projection semantics for bindings that expose
   keyed reactive families.

## Verification

- A Computed observed before dependency publication recomputes exactly once when
  that dependency becomes available.
- Publishing an unrelated key does not recompute it.
- Available-to-available value changes remain reactive.
- Available-to-unavailable and unavailable-to-available transitions preserve
  the same dependency identity.
- Concurrent first observers converge on one slot in the thread-safe and async
  families.
- Removal and scope teardown do not leak slots, tasks, or subscriptions.
- Agent Doc's document-authority graph can replace its compatibility
  materialization path with the Lazily primitive and retain the focused-document
  startup regression.

## Rollout

1. Land the primitive and conformance tests in Lazily.
2. Add binding parity where keyed reactive families are supported.
3. Migrate Agent Doc document authority from explicit `None` slot
   materialization to `observe_dependency`.
4. Remove compatibility membership/materialization workarounds after the minimum
   Lazily version is updated.
