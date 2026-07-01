# lazily-formal

Language-agnostic Lean 4 formal model for the lazily reactive-signals family —
the neutral home formal artifacts every binding depends on equally.

## Architecture

- `LazilyFormal/Primitive.lean` — shared abstract types (`StateId`, `EventId`,
  `ActionId`, `GuardId`, `Configuration`, `GuardResolver`).
- `LazilyFormal/StateMachine.lean` — flat FSM kernel (`State → Event → Option
  State`) + theorems (guard rejection, self-transition, equivalence). Ported
  from lazily-spec; the pure core a single-region chart compiles down to.
- `LazilyFormal/StateChart.lean` — full Harel/SCXML model: `Kind` (atomic /
  compound / parallel / history shallow+deep / final), `Transition`, `Chart`,
  `History`,   `StepResult`; transition selection (enabled, conflict resolution),
  LCA exit/enter sets, descent, history record-on-exit / restore-on-enter, and
  `send` (deterministic by construction). Theorems: `enabled_empty_rejects`,
  `send_actions_empty_when_rejected`, `stepActions_sourcing`,
  `recordHistory_idempotent`, the `*_in_states` well-formedness family
  (incl. `send_cfg_in_states`),   `parallel_region_confluence` (with its
  `sendTaken_eq_enabled_of_pairwise_disjoint` transparency lemma), and
  `single_region_refines_flat_machine` (reject case from pointer
  well-formedness; take case derived from `Chart.Coherent`, the structural
  well-formedness the loose `WellFormed` does not pin down). Chart immutability
  holds by construction (pure Lean; `StepResult` carries no replacement `Chart`).

`send` is a total function, so confluence/determinism is by construction — the
universal guarantee every binding inherits by replaying the shared conformance
fixtures in `lazily-spec/conformance/statechart/`.

## Role

This is the **formal** layer; `lazily-spec` is the **wire** layer. lazily-formal
owns primitive types + the flat kernel + the full Harel chart; lazily-spec owns
the wire protocol + its Lean proofs + the conformance fixtures. lazily-formal is
the executable reference behind the fixtures, not a wire artifact.

## Commands

```bash
make check   # == lake build
```

Pinned to Lean 4.30.0; pure stdlib, no dependencies.

<!-- tsift:code-navigation v=0.1.73 -->
## Code Navigation

Keep this block self-contained for Codex/OpenCode prompt reuse.

Run `tsift status` at session start from the owning repo root. Prefer bounded
digest commands over raw transcript/diff reads; only read full source files when
tsift results are insufficient.

For local verification, run `make check` before committing.
