# Plan: Typed state tables as Lazily Computeds

**Status:** Proposed
**Created:** 2026-08-01
**Session:** `tasks/software/lazily.md`
**Related:** `tasks/software/plan-lazily-state-machine-mirroring.md`

## Problem

Reactive applications commonly derive an effect from several independently changing facts. Hand-written nested conditionals make the valid state space, impossible combinations, and transition coverage difficult to review. The Agent Doc retained-transition incident exposed this directly: an exact unfinished delivery must resume its pinned continuation, while an already-terminal post-commit reposition must settle without replaying that continuation.

Lazily already supplies the propagation graph. The missing piece is a small typed state-table layer that makes the product state and its derived effect exhaustive without creating a second scheduler, persistence layer, acknowledgement protocol, or sidecar.

## Decision

Model a state table as a pure `Computed` over typed input projections:

```text
Computed<InputState> ── transition table ──> Computed<Decision>
                                              │
                                              └── effect sink observes distinct decisions
```

- `InputState` is an enum or a struct containing enums. It names semantic states rather than storing loosely typed labels.
- `Decision` is an exhaustive enum such as `Wait`, `Reject(Conflict)`, `Resume(PinnedContinuation)`, or `Settle(MaterializedTransition)`.
- The transition table is a total pure function. Missing rows fail compilation where the language supports exhaustive matching and fail generated coverage tests elsewhere.
- Effects remain outside the table. An effect receipt records successful publication and feeds back as another observed fact; it is not an imperative request/ACK gate.
- Durable replay reconstructs input projections from ordered domain events. The table itself owns no snapshot, CRDT sidecar, cache, or hidden mutable state.

This complements the existing `StateMachine` roadmap. `StateMachine` owns an accepted sequence of state changes; a state table derives a decision from the current product of observed facts. Either can be implemented using the other internally, but their public semantics should remain distinct.

## Proposed Rust surface

Start in `lazily-rs`, without a procedural macro:

```rust
pub trait StateTable {
    type Input;
    type Decision;

    fn decide(input: &Self::Input) -> Self::Decision;
}

pub fn state_table<T>(
    input: impl Signal<T::Input>,
) -> Computed<T::Decision>
where
    T: StateTable;
```

Use Rust enums for closed alternatives. Rust's `union` is an unsafe overlapping-memory representation, not the equivalent of Python's `A | B` sum type and not suitable for typing label strings. If external strings are required, parse them once into an enum at the boundary and reject unknown labels there.

The first implementation should favor ordinary exhaustive `match` expressions. Add a declarative table builder only if at least three real consumers show that repeated boilerplate obscures the transition relation.

## Coverage contract

For every table:

1. Enumerate each finite input axis and decision variant.
2. Generate or hand-write the Cartesian product test when the state space is tractable.
3. Assert the decision for every reachable row.
4. Assert that impossible combinations are represented explicitly as `Invalid` or rejected while constructing `InputState`.
5. Test both arrival orders for independently propagated facts.
6. Test restart/replay from durable events without consulting current-cycle mutable state.
7. Test effect idempotence and the distinction between attempted and successfully published effects.

Large or unbounded payloads should be classified into a finite semantic enum before entering the table. Do not enumerate hashes, document text, revisions, or continuation bodies.

## Pilot: retained document transition

Extract the Agent Doc retained-transition reducer as the pilot consumer:

| Projected condition | Decision |
|---|---|
| no retained transition | `Idle` |
| required projection facts unavailable | `Wait` |
| visible target exactly matches an unfinished owning cycle | `ResumePinnedContinuation` |
| newer visible projection proves a terminal post-commit reposition materialized | `SettleMaterializedTransition` |
| visible content is malformed or conflicts with lineage | `RejectConflict` |
| an equal-or-newer successful effect frontier already exists | `Settled` |

The input type should group projection, delivery, generation, terminal-cycle, and applied-frontier facts into one named product state. Avoid presence-only locals such as `_projection_present`; represent distinctions that affect a row in the type, and remove observations that do not affect any decision.

## Phases

### Phase 1 — inventory and semantics

- Inventory three candidate consumers in `lazily-rs` and Agent Doc.
- Classify each as state ownership (`StateMachine`) or state derivation (`StateTable`).
- Write the pilot product-state enum and exhaustive table before changing runtime behavior.

### Phase 2 — minimal `lazily-rs` helper

- Implement the trait/helper using existing `Computed` primitives.
- Add no runtime, background task, persistence, macro, or dynamic string registry.
- Verify distinct-until-changed behavior and dependency invalidation.

### Phase 3 — pilot migration

- Migrate the retained-transition reducer without changing its event schema.
- Preserve pinned continuation identity across checkpoint, abandonment, and supervisor recycling.
- Prove terminal materialized transitions settle without supervisor replay.

### Phase 4 — parity

- Specify the table semantics in `lazily-spec`.
- Add equivalent helpers only to full reactive-core ports where the pilot demonstrates value.
- Kotlin, JavaScript, VS Code, JetBrains, and Zed remain projection consumers; they do not acquire local durable state machines merely for API symmetry.

### Phase 5 — ergonomic review

- Evaluate whether a table literal or code generator materially improves three concrete tables.
- Keep exhaustive native enums as the source of truth.
- Reject an API that accepts arbitrary string labels or silently falls through unmatched rows.

## Non-goals

- Fixing every runtime defect merely by enumerating states; the table improves completeness but cannot replace integration, concurrency, and crash-recovery tests.
- Introducing imperative requests or editor acknowledgements into reactive propagation.
- Persisting a second document authority.
- Replacing `Computed`, `Signal`, or the existing Lazily scheduler.
- Mirroring a local state machine into editor adapters that should only publish observations and render projections.

## Completion criteria

- The pilot table is exhaustive and every row has a test.
- `make check` passes in `lazily-rs` and the pilot consumer.
- Arrival-order, restart, and effect-frontier tests pass.
- The public API documents the distinction between state machine, state table, projection, transition, target, and retained transition.
- Cross-language parity is based on semantic capability, including Zed projection parity, rather than identical implementation shape.
