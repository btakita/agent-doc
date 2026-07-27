# Plan — finish the async-plane v2 kernel migration

## Goal

Make `AsyncSource<T>` and `AsyncComputed<T>` the canonical public async value-node
names in every binding, while preserving source compatibility where a binding has
published pre-v2 names.

This is a vocabulary/API migration, not a new interop capability. It must not make
an implementation eligible for a network-suite feature group that its production
surface does not otherwise support.

## Measured baseline (2026-07-26)

| Binding | Current public surface | Work |
|---|---|---|
| Rust | `AsyncSource` / `AsyncComputed`; deprecated handle aliases | Complete |
| JavaScript | `AsyncSource` / `AsyncComputed`; deprecated `AsyncCellHandle` / `AsyncSlotHandle` value and type aliases | Complete |
| Kotlin | `AsyncSource` / `AsyncComputed`; deprecated handle subclasses | Complete |
| C# | `AsyncSource` / `AsyncComputed`; unpublished binding has no legacy handle surface to preserve | Complete for handle names |
| C++ | `AsyncCellHandle` / `AsyncSlotHandle`; `cell`, `slot`, and live `memo` constructors | Migrate |
| Go | `AsyncCellHandle` / `AsyncSlotHandle`; `NewAsyncCell`, `NewAsyncSlot`, and `NewAsyncMemo` | Migrate |
| Python | native classes `AsyncCellHandle` / `AsyncSlotHandle`; canonical map names only | Migrate |
| Zig | async computed handle is `AsyncSlotHandle`; async source identity is represented by the context's cell registry rather than a named public handle | Migrate/clarify |

`AsyncSlotState` is a separate decision. It is the name of the formal four-state
machine in `lazily-formal`, not merely a pre-v2 handle spelling. Do not mechanically
rename the theorem/module vocabulary. If the public API should say
`AsyncComputedState`, add that as the canonical projection with a deprecated
`AsyncSlotState` alias while keeping formal references explicit.

## Compatibility policy

- Published bindings keep deprecated aliases for at least one release line.
- Constructors return the canonical type even when invoked through a deprecated
  constructor alias.
- Unpublished C# does not invent aliases for names it never shipped.
- Deprecated names may appear only in compatibility declarations/tests and
  migration notes. New implementation code, examples, and generated API docs use
  the canonical names.
- `memo` is not a third node kind. It becomes a deprecated guarded-computed
  constructor forwarding to `computed`; equality/ripple policy remains an option
  on `AsyncComputed`.

## Implementation order

### 1. Pin the naming contract

Update the async API section of lazily-spec to name the two async value kinds
`AsyncSource` and `AsyncComputed`, state explicitly that `memo` is a guarded
computed constructor rather than a kind, and settle the public
`AsyncComputedState` versus formal `AsyncSlotState` distinction.

Add a small coverage check that searches public declarations and fails when a
binding has no canonical async source/computed pair. The check must permit old
names only in explicit deprecated compatibility declarations.

### 2. C++

- Rename the implementation structs to `AsyncSource<T>` and `AsyncComputed<T>`.
- Add deprecated `using AsyncCellHandle = AsyncSource` and
  `using AsyncSlotHandle = AsyncComputed` aliases.
- Add canonical `AsyncContext::source` and `AsyncContext::computed` factories.
- Keep `cell` and `slot` as deprecated forwarders.
- Replace `memo` with a deprecated forwarder to the guarded `computed` overload;
  do not retain a separate construction path.
- Convert `AsyncSourceMap` / `AsyncComputedMap` internals and tests to canonical
  handles before testing aliases.

Proof: compile-time same-type assertions for both aliases, factory return-type
assertions, async map tests, and the full C++ gate.

### 3. Go

Go 1.24 is already the module floor, so generic compatibility aliases are
available.

- Rename concrete types to `AsyncSource[T]` and `AsyncComputed[T]`.
- Add deprecated generic aliases:
  `type AsyncCellHandle[T any] = AsyncSource[T]` and
  `type AsyncSlotHandle[T any] = AsyncComputed[T]`.
- Add `NewAsyncSource`, `NewAsyncComputed`, and guarded-computed constructors.
- Keep `NewAsyncCell`, `NewAsyncSlot`, and `NewAsyncMemo` as deprecated forwarders.
- Mechanically migrate queue families, maps, disposal, and conformance models to
  canonical names.

Proof: compile-time assignments through both spellings, disposal and invalidation
tests, async queue/map suites, `go test ./...`, and `go vet ./...`.

### 4. Python

- Rename the native implementation classes to `AsyncSource` and
  `AsyncComputed`.
- Export `AsyncCellHandle = AsyncSource` and
  `AsyncSlotHandle = AsyncComputed` as deprecated compatibility aliases.
- Migrate annotations, `isinstance` checks, reprs, context factories, graph
  conformance adapters, and package exports to canonical names.
- Preserve object identity: an object created through either spelling must pass
  `isinstance` checks for both aliases because they are the same class object.

Proof: explicit alias-identity tests, repr/API tests, async context/property tests,
reactive-graph conformance, mypy/mypyc checks, and the full Python gate.

### 5. Zig

- Rename the public computed-handle type function to `AsyncComputed`.
- Keep `AsyncSlotHandle` as a deprecated source-compatible alias.
- Introduce an explicit `AsyncSource` public handle projection only if it can
  faithfully represent the existing generation-checked cell registry. Do not
  manufacture a wrapper that bypasses disposal or dependency tracking.
- Rename context factories to `source` / `computed`; retain old spellings as
  documented compatibility declarations.
- Decide the public state projection after the spec decision; keep formal
  `AsyncSlotState` references intact.

Proof: compile-time alias equality, stale-generation/disposal tests, dependency
registration, stale-completion discard, async-map conformance, and the full Zig
gate.

### 6. Family cleanup

- Regenerate API docs and update examples only after all four remaining bindings
  expose the canonical pair.
- Search public surfaces for pre-v2 names and classify every remaining hit as a
  deprecated alias, compatibility test, migration note, or formal-model reference.
- Run each binding's local `make check`, commit and push per subrepo, then update
  the root submodule pointers in one explicit commit.

## Network-suite admission

The peer registry advertises capabilities, channels, codecs, and variants from
production behavior—not from type names. This migration changes no capability
bit. In particular, lazily-cs remains registry-visible but staged from unsupported
peer groups until its IPC/sync implementations and canonical fixture replays ship.

The suite may add an informational `api.async_v2_names` capability for API-shape
tests, but it must not use that flag as a proxy for transport, CRDT, queue, or
full-duplex sync support.

## Exit criteria

- Every binding exposes canonical `AsyncSource` and `AsyncComputed` names.
- Published bindings retain tested deprecated aliases.
- No live `memo` path constructs a distinct node kind.
- The public state-name decision is documented and tested without renaming the
  formal model by accident.
- All affected binding gates pass.
- Network-suite membership is unchanged except for the optional API-shape feature
  group.
