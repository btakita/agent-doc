# Plan: Typed state tables as Lazily Computeds

**Status:** Phases 1-3 landed; Phase 4 (cross-language parity) open
**Created:** 2026-08-01
**Session:** `tasks/software/lazily.md`
**Related:** `tasks/software/plan-lazily-state-machine-mirroring.md`
**Implementation:** `lazily-rs/src/state_table.rs`, `lazily-rs/tests/state_table_pilot.rs`, `agent-doc/agent-doc-state-backbone/src/retained_write.rs`

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

## Rust surface (as landed)

In `lazily-rs`, without a procedural macro. The proposal's
`input: impl Signal<T::Input>` does not survive contact with the Cell kernel —
`Signal` is retired, and the eager/lazy axis belongs to `Computed::eager`, not
to the table's signature:

```rust
pub trait StateTable {
    type Input;
    type Decision;

    fn decide(input: &Self::Input) -> Self::Decision;
}

pub fn state_table<T: StateTable>(ctx: &Context, input: Computed<T::Input>) -> Computed<T::Decision>;
pub fn projected_state_table<T: StateTable, F>(ctx: &Context, project: F) -> Computed<T::Decision>;

// The row set, and the assertions over it.
pub trait FiniteState: Sized { fn all() -> Vec<Self>; }
pub fn table_coverage<T: StateTable>() -> TableCoverage<T::Input, T::Decision>;
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

### Phase 1 — inventory and semantics — DONE

Four consumers inventoried and classified:

| Consumer | Classification | Note |
|---|---|---|
| `agent_doc_state_backbone::retained_write::settlement_verdict` | derivation — `StateTable` | Already a total pure function over `Source`s feeding a `Computed<SettlementVerdict>`. Missing both halves of the contract: its input is three `Option`s of hash-bearing structs, so it has no row set; and presence carried by `Option` makes impossible combinations (no retained intent + a divergent visible projection) constructible, ruled out only by the function body. **The pilot.** |
| `coordination::LeaderCell::role` | derivation — `StateTable` | `holder(now)` classified into three rows (none / is-me / is-other). The unbounded peer id is already classified at the boundary; this is the shape, at its smallest. |
| `service::HealthCore::health` | derivation — `StateTable` *after* a projection | Folds a variable-arity probe map, so it has no finite product as written. It becomes one over `(any_critical_down, any_down)` — 4 rows, one of which (`critical down` + `nothing down`) is unconstructible. |
| `resilience::CircuitBreakerCore` (`BreakerState`) | ownership — `StateMachine` | Holds Closed/Open/HalfOpen and folds outcomes into it. Not a table. |

`membership::MembershipCore` (`PeerState`) classifies the same way as the
breaker, for the same reason.

The pilot product-state enum and its exhaustive table are written in
`lazily-rs/tests/state_table_pilot.rs`, ahead of any Agent Doc runtime change.

### Phase 2 — minimal `lazily-rs` helper — DONE

`lazily-rs/src/state_table.rs`: the `StateTable` trait, `state_table` /
`projected_state_table` (plus `thread_safe_*` counterparts), `FiniteState` with
tuple/`Option`/`bool` impls and the declarative `finite_state!` macro, and
`table_coverage` returning a `TableCoverage` row set.

No runtime, background task, persistence, proc-macro, or dynamic string
registry. `projected_state_table` builds **two** guarded cells rather than one —
that is the load-bearing part: payload churn that leaves the product state
unchanged stops at the projection and never reaches `decide`. Mutation-checked
by collapsing them into one cell (the coverage test reddens 7 vs 1) and by
dropping the decision cell's `PartialEq` guard (the effect sink reddens with a
duplicate `Wait`).

Every coverage assertion fails on an empty row set rather than sweeping nothing
(`assert_at_least`, `#lzvacuousrun`), and `assert_at_least(0)` is rejected
outright — a floor that cannot fail is not a floor.

### Phase 3 — pilot migration — DONE

The Agent Doc settlement reducer now classifies its observations into an
eight-row finite semantic sum, derives an exhaustive `SettlementDecision`
through `thread_safe_projected_state_table`, and hydrates the existing public
`SettlementVerdict` in a downstream `Computed`.

- The retained-transition event schema and public verdict API are unchanged.
- Pinned continuation identity remains owned by the existing durable projection
  across checkpoint, abandonment, and supervisor recycling.
- Exact, rebased-payload, superseded-delta, and later-closeout-stage evidence
  each settle through explicit typed rows without supervisor replay.
- Authority/disk divergence and payload absence remain distinct unsettled rows;
  missing observations remain a non-authorizing `Unobserved` row.
- Raw hash churn that does not change the semantic row stops at the guarded
  projection, while the hydrated verdict still carries the current hash.
- Effects and durable receipts remain outside the table. No editor request,
  acknowledgement gate, sidecar runtime, or second authority was introduced.

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

- [x] The pilot table is exhaustive and every row has a test — 11 constructible rows of a naive 32, all six decisions reached, `assert_decisions_exactly`.
- [x] Arrival-order, restart, and effect-frontier tests pass.
- [x] The public API documents the distinction between state machine and state table (`src/state_table.rs` module docs); the pilot names projection, visible classification, frontier, and retained transition.
- [x] `make check` passes in `lazily-rs`.
- [x] `make check` passes in the Agent Doc pilot consumer.
- [ ] Cross-language parity is based on semantic capability, including Zed projection parity, rather than identical implementation shape — pending Phase 4.
