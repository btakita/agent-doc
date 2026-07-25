# lazily-cs — full-parity plan

Goal: bring `src/lazily-cs` to feature parity with the eight sibling bindings, as measured by
`src/lazily-spec/coverage.json` (48 rows) and the conformance corpora under
`src/lazily-spec/conformance/`.

Repo: `git@github.com:lazily-hub/lazily-cs.git` → `src/lazily-cs` (submodule).

## Shipped

- **Row 0 — reactive graph.** `Source<T>` / `Computed<T>` / `Effect`, value-threaded `Compute`
  with a runtime fortification guard, non-consuming mark-frontier invalidation, pull-time
  equality guard, `Eager()`/`Lazy()`, `Batch`, explicit disposal, `TeardownScope` (reverse
  creation order, `Disarm`), edge-degree introspection, bounded effect drain with attribution.
- **Row 15 — merge algebra.** `MergePolicy<T>` with `KeepLatest`/`Sum`/`Max`/`SetUnion`/`RawFifo`,
  `Cell ≡ Source<KeepLatest>`, law tests for associativity and the declared flags.
- **Conformance.** The whole `reactive-graph` corpus replays (20 fixtures, 215 assertions) with an
  empty unsupported/divergence ledger — including the five merge-feed fixtures and the
  bounded-feedback-drain fixture that several sibling runners park as unsupported.

## Method, per phase

Every phase follows the same loop, and none of it is optional:

1. Read the reference implementation in `src/lazily-go` (closest structural match — GC'd,
   interface-dispatched) and `src/lazily-rs` (the semantic reference; its conformance runner is
   the authority on how a fixture is meant to be interpreted).
2. Port the surface into `src/lazily-cs/src/Lazily/`, keeping the family vocabulary. Record any
   deliberate C# divergence in `AGENTS.md` so it is a decision rather than drift.
3. Write the corpus runner for that plane **before** claiming the row. Runners must keep the
   discipline already established in `ReactiveGraphConformanceTests`: never vendor fixtures,
   treat an absent corpus as a hard failure rather than a skip, assert a positive fixture and
   assertion floor, keep two-directional `Unsupported`/`KnownDivergences` ledgers, and install
   library-behaviour counters inside the library's own call path.
4. Mutation-check the new runner: break the thing the fixture exists to catch, confirm the suite
   goes red, restore.
5. `make check` + `dotnet format --verify-no-changes`, commit, push, confirm CI.
6. Flip the row in `src/lazily-spec/coverage.json`, run `node scripts/sync-coverage.mjs`, and
   commit the regenerated table in the spec **and** in each sibling README it touches (one commit
   per repo — `coverage.json` is a hot multi-session file).

A row is only marked shipped when its corpus replays with an empty ledger. A partial port gets
`~`, and the ledger entry says exactly what blocks the rest.

## Phases

| ID | Rows | Scope | Corpora |
|---|---|---|---|
| `#lzcsconcurrency` | 5, 6 | `ThreadSafeContext` (lock-backed) and `AsyncContext`. Re-parameterise the reactive-graph runner over the execution model so all three replay the same corpus — the async cascade defect this corpus exists to catch is invisible to a single-model replay. | reactive-graph (×3 models) |
| `#lzcscollections` | 1–4, 9–11 | `SlotMap` / `ThreadSafeSlotMap` / `AsyncSlotMap` materialization, `ReactiveMap` (`CellMap`/`SlotMap`), `CellTree` + reconcile, `SemTree`, stable-id alignment. | collections, materialization, familysync |
| `#lzcsstatechart` | 7, 8 | Flat state machine, Harel statecharts. | statechart |
| `#lzcsqueues` | 12–14, 16 | `QueueCell` + `QueueStorage`, `TopicCell`, `WorkQueueCell`, `RelayCell` with `BackpressurePolicy`/`SpillStore`/`Transport`. Builds directly on the merge algebra already shipped. | (unit + rateshape overlap) |
| `#lzcscrdt` | 17–24 | `TextCrdt` + delta sync, `SeqCrdt`, `CrdtTree`, `LosslessTreeCrdt` incl. dotted-frontier anti-entropy and concurrent-merge convergence, LWW/MV registers, `PnCounter`, `CellCrdt`. | crdt-tree, lossless-tree |
| `#lzcsipc` | 25–27 | `Snapshot`/`Delta`/`CrdtSync` wire types against the JSON Schemas, `ShmBlobArena`, `BlobBackend` zero-copy transport. Schema validation is the gate; the fixtures at the corpus root are the wire vectors. | (root fixtures) |
| `#lzcssync` | 28–32 | `CrdtPlaneRuntime` + anti-entropy, resync coordinator, durable `OutboxStore`, `IpcSink`/`IpcSource` + full-duplex `SyncDriver`. | reliable-sync, distributed, message-passing |
| `#lzcstemporal` | 40, 41, 45 | `TimerCell`/`IntervalCell`/`CronCell`/`DeadlineCell` over a logical clock, debounce/throttle/sample/probabilistic-sample, tumbling/sliding/session windows. | temporal, rateshape, windowing |
| `#lzcscluster` | 42–44, 46, 47 | `MembershipCell` (SWIM + Phi-accrual), `LeaseCell`/`LeaderCell`/`LockCell`/`SemaphoreCell`/`BarrierCell`, presence/awareness/ephemeral, circuit breaker/retry/bulkhead/timeout, health/readiness/discovery/registry. | membership, coordination, presence, resilience, service |
| `#lzcsplane` | 33–39 | State projection/mirror, `CausalReceipts`, `command-plane-v1`, C-ABI FFI boundary, `PeerPermissions`/`RemoteOp`, `SessionHandshake`, instrumentation + benchmarks. | receipts, signaling, service, agent-doc |

## Sequencing note

`#lzcsconcurrency` goes first deliberately. Every later plane ships sync, thread-safe, and async
variants in the sibling bindings, so the context kinds are a prerequisite rather than a peer —
and the multi-model conformance replay it installs is what stops each subsequent phase from
passing on the sync path while silently broken on the async one.

## Packaging

Tracked as `#lzcspackage`, not a feature row:

- Multi-target `netstandard2.1` / `net8.0` alongside `net10.0` so the package is usable outside
  the newest runtime. `INumber<T>` in the merge algebra is the one constraint that does not exist
  before `net7.0`; it needs a shim or a policy split.
- NuGet publish + a release workflow. Needs a NuGet API key from the operator.

## lazily-formal

The formal model in `src/lazily-formal` is language-agnostic and already covers the reactive
core (for example `disposeScope_eq_disposeAll`, which the `scope_teardown_equals_fold_of_disposals`
fixture is the executable form of). lazily-cs conforms to it *through* the conformance corpus
rather than by carrying its own Lean development — the same relationship every other binding has.
No lazily-formal change is required by this plan.
