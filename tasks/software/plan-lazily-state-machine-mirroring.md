# Plan: State Machine mirroring across the lazily ecosystem

**Status:** Decision + phased roadmap. Design questions resolved; implementation gated on per-phase backlog IDs.
**Created:** 2026-06-24
**Session:** `tasks/software/lazily-rs.md`
**Queue items:** State Machine in lazily-spec / FFI channel spec / SM in lazily-zig / SM in lazily-py / lazily-kt / lazily-js

## Context

The FFI state-projection work (`plan-lazily-ffi-state-projection.md`, `#lzffistate`/`#lzffistate1`) established that the binary owns durable FSMs and plugins consume serializable projections. This plan extends that foundation: formalize `StateMachine` in the Lean spec, then achieve StateMachine parity across ports and create the thin FFI-consumer packages (`lazily-kt`, `lazily-js`).

## Parity matrix (current)

| Port | Slot | Cell | Signal | StateMachine | IPC | Lean |
|---|---|---|---|---|---|---|
| lazily-rs | yes | yes | yes | yes (x3: sync/thread-safe/async) | yes | — |
| lazily-py | yes | yes | yes | **no** | yes | — |
| lazily-zig | yes | yes | **no** | **no** | yes | — |
| lazily-spec | — | — | — | **StateMachine.lean (new)** | IPC.lean | this IS the formal layer |
| lazily-kt | — | — | — | — | — | — |
| lazily-js | — | — | — | — | — | — |

## Decision

1. **StateMachine belongs in lazily-spec.** `StateMachine.lean` formalizes the `Fn(&S, &E) -> Option<S>` transition/guard model, proving: guard rejection preserves state, accepted transitions advance, self-transitions with `PartialEq` suppress propagation. This is the first formal linkage between the reactive layer and the transport layer (IPC.lean).

2. **FFI channel spec is deferred.** The FFI channel (`LazilyFfiChannel`) is a FIFO byte pipe — its semantics are trivial. The interesting proofs (encode/decode round-trip, frame ordering) are mechanical. The IPC.lean Delta epoch theorems already cover the substantive ordering invariants. Revisit if a non-trivial FFI invariant emerges.

3. **lazily-kt and lazily-js are FFI-consumer packages, NOT reactive-core ports.** Per the no-port decision (`plan-lazily-ffi-state-projection.md`): they wrap the JNA/native-addon bridge, hold projections in platform-native reactive holders (`StateFlow` / signal+`EventEmitter`), and report editor events. Zero reactive graph, zero transition logic. The `StateMachine` they "mirror" is consumed as a projection value, never executed locally.

4. **StateMachine in lazily-zig and lazily-py is legitimate port-primitive work.** Unlike a full reactive-core port, `StateMachine` is a single self-contained primitive (state cell + pure transition fn). Both ports already have `Cell`, so adding `StateMachine` is additive feature parity — the same primitive, not a re-implementation of the propagation graph.

## Phases

### Phase 1 — `StateMachine.lean` formal spec (`#lzsmlean`)
- DONE (initial): `LazilyFormal/StateMachine.lean` — models `Machine` (current + transition), `send`, `sends`, with guard-rejection / accepted-transition / self-transition theorems.
- TODO: add Lean proof linking a `send` to an IPC `DeltaOp.cellSet` emission (cross-module: `StateMachine` → `IPC`), proving a materialized transition is observable on the wire.
- `make test-lean-formal` must pass.

### Phase 2 — StateMachine in lazily-py (`#lzsmpy`)
- Add `src/lazily-py/src/lazily/state_machine.py`: `StateMachine(initial, transition)` wrapping a `Cell`, with `send(event) -> bool` and `state` property.
- Mirror the lazily-rs API: transition returns `Optional[S]`, `None` = guard rejection.
- Tests: `tests/test_state_machine.py` — transition acceptance, guard rejection, self-transition no-op, reactive invalidation on change.

### Phase 3 — StateMachine in lazily-zig (`#lzsmzig`)
- Add `src/lazily-zig/src/lazily/state_machine.zig`: `StateMachine(S, E)` wrapping a Cell + transition function pointer.
- Zig has no closures; transition is `*const fn(*const S, E) ?S`.
- Tests: transition acceptance, guard rejection.

### Phase 4 — lazily-kt FFI-consumer package (`#lazilyst`) (`#lzkt`)
- Kotlin package under `src/lazily-kt/` (Gradle, `maven-publish`).
- Wraps JNA bridge to `lazily_ffi_*` + agent-doc FFI exports.
- `StateFlow<DocumentStateProjection>` projection holder; `recordStateEvent(fact)` event reporter.
- `*.d.ts` equivalent: Kotlin type-safe API, no reactive core.
- Blocked on `#lzffistate1` (the FFI state-projection exports must exist first).

### Phase 5 — lazily-js FFI-consumer package (`#lzjs`)
- JavaScript + TypeScript package under `src/lazily-js/` with `*.js` + `*.d.ts`.
- Wraps the VSCode native addon (N-API) binding to `lazily_ffi_*`.
- Signal/`EventEmitter` projection holder; event reporter.
- Zero reactive graph; pure FFI consumer + projection renderer.
- Blocked on `#lzffistate1`.

## Non-goals
- No reactive-core port (Context/propagation graph) in Kotlin or TypeScript.
- No local durable FSM execution in lazily-kt/lazily-js — projections only.
- No ThreadSafeStateMachine/AsyncStateMachine in lazily-py or lazily-zig (single-threaded variants only, matching their existing concurrency model).
