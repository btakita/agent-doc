# lazily-formal

Language-agnostic **Lean 4** formal model for the lazily reactive-signals
family — the neutral home for the formal artifacts every binding depends on
*equally*. Not owned by `lazily-rs` (which would create Rust-centrism) and not
buried in `lazily-spec` (a wire-protocol repo).

`lazily-formal` is the **formal** layer; `lazily-spec` is the **wire** layer.
This repo owns the executable reference *behind* the cross-language
state-chart conformance fixtures: a total, deterministic `send` whose type is
itself the confluence proof that all bindings agree on *every* input, not just
the tested ones.

## Architecture

Three modules, layered primitive → flat kernel → full chart:

- **`LazilyFormal/Primitive.lean`** — shared abstract types
  (`StateId`, `EventId`, `ActionId`, `GuardId`, `Configuration`, `GuardResolver`).
  Intentionally abstract `Nat` stand-ins: the model fixes *behavior*, not wire
  encoding — the wire encoding is fixed by `lazily-spec` conformance fixtures.
- **`LazilyFormal/StateMachine.lean`** — the flat FSM kernel
  (`State → Event → Option State`) + theorems (guard rejection, self-transition,
  `sends` flag, transition preservation). Ported from lazily-spec so this repo
  is the canonical home for the kernel. The pure transition core a
  single-region chart compiles down to, and the layer whose transitions surface
  on the IPC wire.
- **`LazilyFormal/StateChart.lean`** — the full Harel/SCXML model: `Kind`
  (atomic / compound / parallel / history shallow + deep / final), `Transition`,
  `Chart` + `WellFormed`, `History`, `StepResult`; transition selection
  (`enabled`, conflict resolution via disjoint exit sets), LCA exit/enter sets,
  descent, history record-on-exit / restore-on-enter, and `send`.

## Scope — what is modeled

`StateChart` models the full chart subset of `lazily-spec/docs/state-charts.md`:

- Compound (hierarchical) states with default initial descent.
- Orthogonal (parallel) regions.
- Shallow **and** deep history — record-on-exit / restore-on-enter.
- Entry, exit, and transition actions (exit innermost-first → transition →
  entry outermost-first).
- Named guards, resolved host-side (`GuardResolver`); fail-closed when absent.
- External **and** internal transitions (LCA-based exit/enter sets).

Out of scope here (handled elsewhere): the wire encoding, CRDT/IPC semantics,
and `run` actions / `{"expr": …}` context guards / `final`-completion (`done`)
— the model fixes the *behavioral core*, not the host-evaluated surface.

## What is proved

Lean proofs (`make check` builds all of them). Grouped by guarantee:

**Flat kernel (`StateMachine`)**
- `guard_rejection_preserves_state` — a `none` transition leaves the state unchanged.
- `accepted_transition_advances_state` — an accepted `some next` advances to `next`.
- `self_transition_preserves_state` — a self-targeted transition preserves state.
- `self_transition_sends_false` / `changed_transition_sends_true` — the `sends`
  flag matches whether the state actually changed.
- `send_preserves_transition` — `send` never changes the transition function.

**Rejection & actions (`StateChart`)**
- `enabled_empty_rejects` — no enabled, guard-passing transition ⇒ configuration
  and history unchanged.
- `send_actions_empty_when_rejected` — a rejected `send` fires no actions.
- `stepActions_sourcing` — every fired action is sourced from an exited state's
  `exit`, a taken transition's `action`, or an entered state's `entry`.

**Well-formedness invariants (`StateChart`, the `*_in_states` family)**
Under `WellFormed`, every computed set stays inside the chart's declared
`states`: `children_in_states`, `activeLeaves_in_cfg`, `ancestors(Aux)_in_states`,
`enterSubtree_in_states`, `pathBelow_in_states`, `restoreStart_in_states`,
`restoreViaHistory_in_states`, `regionOf_in_states`, `enterSet_in_states`,
`enabledForLeafAux_in_states`, `enabled_in_states`, and
`send_cfg_in_states` (a `send` over a well-formed chart keeps the configuration
inside `states`).

**Conflict-resolution transparency (`StateChart`)**
- `sendTaken_subset_enabled` — the conflict-resolving `keepTrans` fold only
  drops elements, never invents them.
- `keepTrans_appends_of_disjoint` / `foldl_keepTrans_appends_all` — when an
  enabled transition's exit set is disjoint from the accumulator, it is appended.
- `sendTaken_eq_enabled_of_pairwise_disjoint` — under pairwise-disjoint exit
  sets, `keepTrans` is transparent: every enabled transition is taken.

**History (`StateChart`)**
- `recordHistory_idempotent` — recording history for the same exit set twice is
  the same as recording once.

**Parallel-region confluence (`StateChart` — the headline universal result)**
- `parallel_region_confluence` — when enabled transitions are pairwise
  non-conflicting (orthogonal regions, i.e. disjoint LCA exit sets), every
  enabled transition is taken and the resulting configuration depends only on
  the enabled *set*, not its order. Invariant under any reordering of `enabled`.
  Backed by the order-free membership lemmas `mem_zip_fst`, `zip_map_eq`,
  `filter_notContains_mem`, `applyTakenCfg_mem_iff`, `perm_flatMap_mem`, and
  `send_cfg_eq_applyTakenCfg_take`.

**Single-region refinement (`StateChart`)**
- `single_region_enabled_at_most_one` — with one active leaf, at most one
  transition is enabled.
- `single_region_refines_flat_machine` — a single-region chart's `send` refines
  the flat `StateMachine` kernel derived in `flatMachine`: the new active leaf
  equals the flat machine's transition target.
  - Reject case: from pointer well-formedness.
  - Take case: derived from `Chart.Coherent` — the structural well-formedness
    (kinds match the tree; default descents resolve to a unique leaf; the root
    is a common ancestor) that the model's loose `WellFormed` does not pin down.
    All four take-case consequences are proven from the three `Coherent` fields
    (no transition-specific hypothesis).

`parallel_region_confluence` is unconditional modulo the pairwise-disjoint
hypothesis; `single_region_refines_flat_machine` is proved under `Chart.Coherent`
(reject case needs no `Coherent`).

### By construction (not a theorem, but the strongest guarantee)

- **Determinism** — `send` is a total function of
  `(chart, history, configuration, event, guard resolver)`, so a given input
  tuple yields a unique `StepResult`. This confluence guarantee is what no
  finite fixture suite can establish.
- **Chart immutability** — Lean is pure, and `send` returns a `StepResult`
  (`cfg` / `history` / `actions`) that carries no replacement `Chart`; the chart
  definition simply cannot be mutated by a step.

## Role vs. lazily-spec and the bindings

`lazily-formal` is the **formal** layer; `lazily-spec` is the **wire** layer.

| Repo | Owns |
|------|------|
| `lazily-formal` (this) | formal models: flat kernel + full Harel chart; universal proofs |
| `lazily-spec` | wire protocol + JSON schemas + IPC/CRDT Lean proofs + conformance fixtures (incl. `conformance/statechart/`) |
| `lazily-rs` / `lazily-py` / `lazily-zig` / `lazily-kt` / `lazily-js` / `lazily-dart` | native implementations; replay the shared conformance fixtures |

## Verify

```bash
make check   # == lake build
```

Pinned to Lean 4.30.0 via `lean-toolchain`. No external dependencies (pure stdlib).
