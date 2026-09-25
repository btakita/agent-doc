# Reactive Topology Proof Boundary

## Architecture Contract

### Invariant

Every admitted side effect descends from a current observation through the
same lifetime-scoped Lazily graph, executes at most once for that observation
generation, and publishes only an exact-generation receipt. A stale effect or
an effect whose scope has closed cannot mutate external state.

### Policy owner

`agent-doc-state-scope` owns graph lifetime and connectivity. Each domain state
machine remains the sole owner of its exhaustive transition policy; adapters
may publish typed facts and execute returned effects, but may not re-decide the
policy.

### Transition table

| Scope | Observation | Derived generation | Pending effect | Decision |
| --- | --- | --- | --- | --- |
| open | newer | stale | any | recompute; do not execute stale effect |
| open | current | none | receipt behind | schedule one effect |
| open | current | exact | receipt behind | execute once and publish exact receipt |
| open | current | stale | any | discard stale effect, then recompute |
| open | current | none | receipt current | quiescent |
| closed | any | any | any | discard; no mutation or receipt publication |

### Evidence inputs

- Scope-open state.
- Monotone observation generation.
- The generation captured by the current `Computed` decision.
- The generation captured by a pending `Effect`.
- The latest exact effect receipt generation.

### Reactive topology

`observation Source -> policy Computed -> idempotent Effect -> receipt Source`

The receipt invalidates downstream computations in the same typed scope. Scope
drop is teardown; it is not a separately remembered deregistration action.

### Imperative extraction audit

The workspace architecture guard rejects unreviewed ad-hoc Lazily contexts and
deprecated pull-style signal APIs. Domain adapters may still perform external
I/O inside an `Effect`; actorless one-shot CLI probes remain direct because
they have no live graph lifetime to join.

### Allowed edit surfaces

- `formal/tla/ReactiveTopology.{tla,cfg}` for the finite composition proof.
- `formal/tla/README.md` and `scripts/run_tla.sh` for executable proof wiring.
- `specs/process-topology.md` for the normative proof boundary.

### Verification

- TLC invariants: derived and receipt generations never lead observations;
  mutations have exact current lineage; stale/closed effects never mutate;
  each generation mutates at most once; every receipt came from a mutation.
- TLC liveness: once observations stop advancing in an open scope, fair graph
  execution eventually publishes the matching receipt.
- Existing Rust architecture guard and shared-scope invalidation tests connect
  the abstract topology to production construction rules.
- Full `make check`.

### Out of scope

- A universal proof of arbitrary Rust, editor, tmux, filesystem, or network
  implementations.
- Unbounded liveness without fairness or eventual quiescence assumptions.
- Replacing domain-specific authority, closeout, CRDT-lineage, or supervisor
  models with one over-broad model.

