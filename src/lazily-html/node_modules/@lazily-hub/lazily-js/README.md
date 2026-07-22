# lazily-js

Native JavaScript port of the **lazily** reactive family. `@lazily-hub/lazily-js`
is the JavaScript/TypeScript binding alongside [`lazily-rs`][rs],
[`lazily-py`][py], [`lazily-zig`][zig], [`lazily-kt`][kt], and
[`lazily-dart`][dart]. It ships:

- a full reactive graph — the Cell kernel (`Context`, `Source`,
  `Computed`, `Effect`; eager = an eager `Computed`, `computed().eager()`);
- an async reactive graph (`AsyncContext`) for Promise-driven derivations, with
  revision-guarded stale-completion discard, in-flight deduplication, and
  cancellation;
- the [`lazily-spec`][spec] IPC wire types (`Snapshot`, `Delta`, `CrdtSync`,
  capability negotiation, and default-deny peer permissions);
- keyed cell collections, move-minimized reconciliation, and a memoized
  semantic tree;
- move-aware sequence CRDT, Fugue/RGA text CRDT, and manufactured text identity;
- a Cell-backed flat state machine plus a full Harel/SCXML state-chart
  interpreter with the typed `ChartBuilder` API;
- the distributed plane — the WebSocket signaling protocol + client, a
  `DataChannel` transport seam with permission-filtering sink/source, a browser
  `RTCPeerConnection` adapter, and the `CrdtPlaneRuntime` anti-entropy engine;
- a koffi-backed state-projection consumer for agent-doc host projections.

> **Package note.** Earlier `@lazily-hub/js` releases were only a state-projection
> consumer. `@lazily-hub/lazily-js` is the current full reactive binding.

Pure ES modules. The reactive, IPC, collections, CRDT, state-machine, and
state-chart modules have no runtime dependencies; `koffi` is loaded only when
the FFI projection transport is used.

## Feature Set

The full `lazily` capability set across every binding. Legend: ✅ shipped ·
`~` partial · `—` absent or not applicable. The canonical matrix with per-cell
notes and platform carve-outs lives in
[`lazily-spec` § Cross-Language Coverage](https://github.com/lazily-hub/lazily-spec/blob/main/docs/coverage.md).

<!-- coverage-table:start -->
| Feature | Rust | Python | Kotlin | JS | Dart | Zig | Go | C++ |
| --------- | :----: | :------: | :------: | :--: | :----: | :---: | :--: | :---: |
| Reactive graph — two cell kinds (nodes `SourceCell` / `ComputedCell`; handles `Source<T, M>` / `Computed<T>`) + `Effect` sink + eager `Computed` (`computed().eager()`) / all cells guarded / batch | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Keyed-map materialization (`SlotMap`) — mint-on-access derived slots: transparency + deferral (`#lzmatmode`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Thread-safe keyed map (`ThreadSafeSlotMap`) — `Send + Sync` + materialization confluence (`#lzmatmode`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Async keyed map (`AsyncSlotMap`) — eventual transparency (`#lzmatmode`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Keyed-map sync — membership propagation + materialize-on-ingest + derived-aggregate transparency (`#lzfamilysync`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Thread-safe context (lock-backed) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Async reactive context | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Flat state machine | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Harel state charts | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Keyed reactive maps (`ReactiveMap`: `CellMap` / `SlotMap`) + `CellTree` + reconcile | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Memoized semantic tree (`SemTree`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Stable-id alignment (manufactured identity) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Reactive queue (`QueueCell` SPSC/MPSC + `QueueStorage` adapter) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Broadcast topic (`TopicCell`) — independent cursors + durable replay + safe GC (`#lztopiccell`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Competing-consumer work queue (`WorkQueueCell`) — exclusive leases + ack/nack + redelivery + DLQ (`#lzworkqueue`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Merge algebra + `Source<T, M>` — associative `MergePolicy` (`KeepLatest`/`Sum`/`Max`/`SetUnion`/`RawFifo`), `Cell ≡ Source<KeepLatest>`, read-any-cell/write-`Source` split (`#relaycell`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| RelayCell — conflating relay + `BackpressurePolicy` + `SpillStore` + `Transport` + Inbox/Outbox + Rate/Window/Expiry/Priority/keyed policies (`#relaycell`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Free-text character CRDT (`TextCrdt`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `TextCrdt` delta sync (`version_vector` / `delta_since` / `apply_delta`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `CrdtTree` lossless document contract (`#lzcrdttree`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Move-aware sequence CRDT (`SeqCrdt`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Lossless tree CRDT core (`LosslessTreeCrdt`, M1) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Lossless tree — dotted-frontier anti-entropy | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Lossless tree — concurrent merge convergence | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Registers (LWW / MV) + `PnCounter` + `CellCrdt` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| IPC wire — `Snapshot` + `Delta` + `CrdtSync` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Shared-memory blob path (`ShmBlobArena`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Cross-process zero-copy transport (`BlobBackend` / shm / arrow) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Distributed CRDT plane (`CrdtPlaneRuntime` / anti-entropy) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Reliable sync — resync coordinator + at-least-once durable outbox + OR-set/LWW liveness (`#lzsync`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Storage-independent durable outbox (`OutboxStore` + shared outbox protocol; SQLite/Room/IndexedDB/file adapters) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Reliable-sync transport seam + full-duplex `SyncDriver` loop (`IpcSink`/`IpcSource`, `#sync-driver`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Distributed plane — WebRTC transport + signaling | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| State projection / mirror | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Causal receipts (`CausalReceipts` outcome projection) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Message-passing + RPC command plane (`command-plane-v1`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| C-ABI FFI boundary | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Permission boundary (`PeerPermissions` / `RemoteOp`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Capability negotiation (`SessionHandshake`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Instrumentation / benchmarks | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Temporal sources — `TimerCell` / `IntervalCell` / `CronCell` / `DeadlineCell` over a logical clock (`#lztime`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Rate-shaping operators — `DebounceCell` / `ThrottleCell` / `SampleCell` / `ProbabilisticSampleCell` (`#lzrateshape`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Membership + failure detection — `MembershipCell` (SWIM + Phi-accrual) / `PeerSet` / `PeerChangeEvent` (`#lzmemb`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Distributed coordination — `LeaseCell` / `LeaderCell` / `LockCell` / `SemaphoreCell` / `BarrierCell`+`QuorumCell` (`#lzcoord`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Presence + ephemeral plane — `PresenceCell` / `AwarenessCell` / `EphemeralCell` + `Ephemeral`/`Durable` markers (`#lzpresence`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Stream windowing — `TumblingWindow` / `SlidingWindow` / `SessionWindow` over the merge algebra (`#lzwindow`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Fault tolerance — `CircuitBreakerCell` / `RetryPolicyCell` / `BulkheadCell` / `TimeoutCell` (`#lzresilience`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Embedded-service plane — `HealthCell` / `ReadinessCell` / `DiscoveryCell` / `ServiceRegistry` (`#lzservice`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
<!-- coverage-table:end -->

Two JS ✅ marks are backed by runtime-specific mechanisms while keeping the core isomorphic:

- **Thread-safe** (`ThreadSafeReactiveMap`, `Thread-safe context`) — cross-realm mutual exclusion via a `SharedArrayBuffer` + `Atomics` reentrant mutex shared across Web Workers / `worker_threads`; degrades to a single-realm guard where shared memory is unavailable (e.g. a browser without cross-origin isolation), which is sound because no shared memory means no cross-realm concurrency.
- **C-ABI FFI** — the normative codec + in-process `FfiChannel` are pure-JS and run unchanged in the browser; the Node build additionally binds the real `lazily` shared library (`lazily_ffi_channel_*`) via koffi. Both speak the identical byte contract, so browser and native are drop-in interchangeable.

CRDT convergence and the wire protocol are pinned by the shared conformance fixtures
and JSON Schemas in `lazily-spec` and the Lean models in `lazily-formal`.
## Packages

| Import | What it is |
|--------|------------|
| `@lazily-hub/lazily-js` | `lazily-spec` IPC wire types: `Snapshot`, `Delta`, `DeltaOp`, `IpcMessage` (`Snapshot` / `Delta` / `CrdtSync`), `NodeState`, `IpcValue`, `PeerPermissions`, `SessionHandshake`, `BINDING_CAPABILITIES` |
| `@lazily-hub/lazily-js/transport` | Cross-process zero-copy transport (`#lzzcpy`): `ShmBlobArena`, `InProcessBackend` / `ArrowBackend`, `BlobRouter`, `spillMessage` / `resolveValue`, and the FFI-gated `createShmBackend` (Node/Bun/Deno). Isomorphic — no FFI import; browser-safe |
| `@lazily-hub/lazily-js/reactive` | Reactive dependency graph — the Cell kernel: `createContext` (alias `Context`), `Source`, `Computed`, `Effect` (eager = an eager `Computed`, `computed().eager()`). Closure-based core (#lzjsclosure) — 2-8x faster reads than the prior class implementation |
| `@lazily-hub/lazily-js/reactive-async` | Async reactive graph: `AsyncContext` — Promise-driven slots/effects with revision-guarded stale-completion discard, in-flight dedup, and cancellation |
| `@lazily-hub/lazily-js/reactive-family` | Unified keyed reactive map: `ReactiveMap<K,V,H>` (reactive membership/order, `getOrInsertWith` mint-on-access, `remove`, `move`) + `CellMap` (adds cell-only `set` + eager `entry`/`entryWith`) and `SlotMap` (lazy `getOrInsertWith` + eager `materializeAll`; no `set`) specializations. No eager/lazy mode flag (`#reactivemap`) |
| `@lazily-hub/lazily-js/async-reactive-family` | Async keyed reactive map: `AsyncReactiveMap` + `AsyncCellMap` / `AsyncSlotMap` over `AsyncContext` — eventual transparency (a pending slot observes `undefined` and resolves to the canonical value; eager ≡ lazy once resolved) (`#reactivemap`) |
| `@lazily-hub/lazily-js/thread-safe` | Lock-backed reactive context: `ThreadSafeContext` (`Send + Sync` flavor of `Context`) + `AtomicMutex` — a real `SharedArrayBuffer` + `Atomics` reentrant mutex giving cross-worker mutual exclusion; degrades to a single-realm guard where shared memory is unavailable |
| `@lazily-hub/lazily-js/thread-safe-reactive-family` | Thread-safe keyed reactive map: `ThreadSafeReactiveMap` + `ThreadSafeCellMap` / `ThreadSafeSlotMap` — mutex-guarded present set with first-writer-wins materialization confluence (`#reactivemap`) |
| `@lazily-hub/lazily-js/ffi` | C-ABI FFI boundary (`schemas/ffi.json`): message codec (`encodeMessage` / `decodeMessage` / `validateMessage` / `kindOf`, `LazilyFfiStatus` / `LazilyFfiMessageKind`) + `FfiChannel` FIFO. Isomorphic core (browser shim); the Node build additionally exposes `NativeFfiChannel` / `loadNativeChannel` over the real `lazily_ffi_channel_*` C ABI via koffi |
| `@lazily-hub/lazily-js/instrumentation` | In-library instrumentation/benchmark API: `benchmark`, `runBenchmarkSuite`, `BenchmarkResult`, `withInstrumentation` — plus opt-in reactive-core counters via `new Context({ instrument: true })` / `instrumentationSnapshot()` |
| `@lazily-hub/lazily-js/state-machine` | Flat finite-state-machine kernel backed by a reactive `Cell` |
| `@lazily-hub/lazily-js/statechart` | Harel/SCXML chart interpreter plus `ChartBuilder`, `StateBuilder`, `TransitionBuilder` |
| `@lazily-hub/lazily-js/collections` | `CellMap`, `CellTree`, keyed reconciliation, and LIS move minimization |
| `@lazily-hub/lazily-js/sem-tree` | Memoized semantic tree over `CellTree`-shaped data |
| `@lazily-hub/lazily-js/seq-crdt` | Move-aware sequence CRDT using independent LWW value / position / deletion registers |
| `@lazily-hub/lazily-js/text-crdt` | Fugue/RGA character CRDT |
| `@lazily-hub/lazily-js/stable-id` | Manufactured text identity: anchors, content hashes, similarity alignment |
| `@lazily-hub/lazily-js/signaling` | WebSocket signaling protocol: `ClientMessage` / `ServerMessage`, `SignalingClient`, `SignalingRoom` routing (anti-spoof, roster), `SignalingPermissions` |
| `@lazily-hub/lazily-js/distributed` | Distributed plane: `DataChannel` seam + `InMemoryDataChannel`, `WebRtcSink` / `WebRtcSource`, `CrdtPlaneRuntime` anti-entropy, and the browser `RTCPeerConnection` adapter |
| `@lazily-hub/lazily-js/state-projection` | koffi FFI consumer for agent-doc `DocumentStateProjection` |

## Reactive graph

`Context` (alias `createContext`, from `@lazily-hub/lazily-js/reactive`) mirrors
the single-threaded lazily-rs `Context` semantics in native JavaScript. The
reactive core is implemented with the **closure factory technique** (#lzjsclosure,
rmemo-style): `createContext()` returns an object whose methods close over
captured graph state, and nodes are plain objects with a numeric discriminator
replacing `instanceof`. V8 inlines these small monomorphic closures more
aggressively than `class` + `#private` methods, so the read/invalidate hot paths
run 2-8x faster than the prior class implementation (see `bench/context.bench.mjs`
and `BENCHMARKS.md`). Both `createContext()` and the historical `new Context()`
are the same function — an alias, not a wrapper.

The family is the **Cell kernel** (`#lzcellkernel`, naming v2) — `Cell` is the
value-node *concept* over two value kinds, and the bare kind name is the
**handle** a caller holds; `Effect` sits outside the hierarchy:

- **Source** — a value written from outside (`ctx.source(v)`); exposes
  `get`/`set`/`merge`. `ctx.source(v, policy)` folds `.merge` under an associative
  `MergePolicy`, so `Source` subsumes the former `MergeCell` (a keep-latest
  `Source` is the plain cell). (v1 `SourceCell`.)
- **Computed** — a value computed from upstream (`ctx.computed(f)`); exposes
  `get` and **not** `set`/`merge`. `computed` is **always guarded**
  (an equal recompute suppresses downstream — matches TC39 `Signal.Computed`).
  (v1 `FormulaCell`.)
- **Effect** — side-effecting observer with cleanup; a value-less sink. (v1
  `EffectHandle`.)

**The read/write split is by method presence.** JavaScript has no compile-time
kind gate and (by design) no runtime one, so a `Source` object simply has
`set`/`merge` and a `Computed` object does not.

**Eager is an eager computed, not a kind.** `ctx.computed(f).eager()` attaches a
puller `Effect` that keeps the computed materialized as soon as a dependency
invalidates; `.eager()` is idempotent and returns the same handle, `.lazy()`
reverts it, and `.isEager()` queries the state. This retires the former `Signal`.

Dependencies are discovered dynamically while a computed/effect computes.
Invalidation is pull-based and glitch-free; every computed uses an equality
guard to suppress downstream work when a recompute produces the same value.
`batch` coalesces invalidations and effect reruns.

```js
import { createContext } from "@lazily-hub/lazily-js/reactive";

const ctx = createContext(); // idiomatic; `new Context()` is the same call
const a = ctx.source(2);
const b = ctx.source(3);

const sum = ctx.computed(() => a.get() + b.get());
sum.get(); // 5

a.set(10);
sum.get(); // 13, recomputed lazily on read

const parity = ctx.computed(() => (a.get() % 2 === 0 ? "even" : "odd")).eager();
a.set(11);
parity.get(); // "odd", already materialized (eager)
```

`memo` and the unguarded `computed` are removed (folded into the guarded
`computed`); `SourceCell`/`FormulaCell`/`CellHandle`/`SlotHandle` handle names
are retired. The `cell`/`slot`/`signal` constructors and the functional
`ctx.get(handle)` / `ctx.set(source, value)` are the canonical accessors;
`getCell` / `setCell` and `cell` / `slot` remain deprecated compatibility aliases.

## Async reactive context

`AsyncContext` (from `@lazily-hub/lazily-js/reactive-async`) is a **separate**
reactive surface for derivations whose values are produced by `async` functions.
It is not an overload of the synchronous `Context`: futures introduce in-flight
state, stale completion, cancellation, and dependency tracking across `await`
that the synchronous graph does not have. Cells remain the synchronous input
layer; computed slots, memos, and effects are async.

Each async slot runs an `Empty → Computing → Resolved/Error` state machine with
**revision-guarded publish** (a completion is published only if the slot's
revision is still current, so a stale result is discarded), **in-flight
deduplication** (concurrent `getAsync` callers share one compute), and
cooperative cancellation. Async effects serialize reruns and always run the
previous cleanup before the next body.

```js
import { AsyncContext } from "@lazily-hub/lazily-js/reactive-async";

const ctx = new AsyncContext();
const userId = ctx.source(1);

const profile = ctx.computedAsync(async (cctx) => {
  const id = cctx.get(userId); // dependency registered before the await
  return await fetchProfile(id);
});

await ctx.getAsync(profile); // spawns the compute, awaits the value
ctx.get(profile); // synchronous cached read once resolved (undefined while pending)

ctx.set(userId, 2); // supersedes any in-flight compute; slot re-resolves
await ctx.getAsync(profile); // the profile for user 2
```

## Keyed reactive maps (`ReactiveMap` / `CellMap` / `SlotMap`)

`ReactiveMap<K, V, H>` (from `@lazily-hub/lazily-js/reactive-family`) is the ONE
unified **keyed reactive collection** (`#reactivemap`): reactive membership +
order, `getOrInsertWith` mint-on-access, `remove`, and atomic `move`, generic
over the entry's handle kind. Its two specializations are the concrete types you
use:

- **`CellMap<K, V>`** — input-cell entries. Adds cell-only `set(key, value)` (an
  input is settable) and eager value-minting (`entry` / `entryWith`).
- **`SlotMap<K, V>`** — derived-slot entries. `getOrInsertWith(key, factory)`
  mints a slot on **first access** ("materialize on pull", **lazy**);
  `materializeAll(keys, factory)` pre-mints the keyset up front (**eager**). A
  slot's value is derived, so `SlotMap` has **no `set`**.

There is **no eager/lazy mode flag** — eager is the pre-mint loop, lazy is
mint-on-access, and they are **observationally transparent**: a read returns the
same value either way; only allocation timing and memory change. Lazy pays off
only for sparsely-touched large keyed address spaces.

```js
import { Context } from "@lazily-hub/lazily-js/reactive";
import { SlotMap } from "@lazily-hub/lazily-js/reactive-family";

const ctx = new Context();

// A derived (slot) map of key*3 over a large address space, built lazily:
// nothing is allocated until a key is read.
const map = new SlotMap(ctx);
map.presentCount(); // 0

map.getOrInsertWith(5, (k) => k * 3); // 15 — first read materializes just this entry
map.presentCount(); // 1
map.isPresent(5); // true
map.isPresent(6); // false

// Eager pre-mints the same values up front — observationally identical.
const eager = new SlotMap(ctx);
eager.materializeAll([0, 1, 2, 3], (k) => k * 3);
eager.get(2) === map.getOrInsertWith(2, (k) => k * 3); // true
```

## State machine and state charts

`StateMachine` is the flat finite-state-machine kernel: a pure
`(state, event) -> nextState | null` transition backed by a reactive `Cell`.
Accepted self-transitions to an equal state are suppressed by the Cell equality
guard.

```js
import { Context } from "@lazily-hub/lazily-js/reactive";
import { StateMachine } from "@lazily-hub/lazily-js/state-machine";

const ctx = new Context();
const light = new StateMachine(ctx, "Red", (state, event) =>
  event === "advance" ? { Red: "Green", Green: "Yellow", Yellow: "Red" }[state] : null,
);

light.send("advance"); // true
light.state; // "Green"
```

`StateChart` implements the full Harel/SCXML subset from
[`lazily-spec/docs/state-charts.md`][statecharts]: compound states, parallel
regions, shallow/deep history, entry/exit/transition actions, named guards
(fail-closed), internal/external transitions, and final leaves. Charts are
**compute, not protocol**: the chart itself is not serialized as a special wire
kind; only an application-level active configuration would cross IPC as ordinary
payload state.

The normative definition path is the declarative JSON chart consumed by the
shared conformance fixtures:

```js
import { ChartDef, StateChart } from "@lazily-hub/lazily-js/statechart";

const def = ChartDef.fromChart({
  initial: "root",
  states: {
    root: { parallel: true },
    flow: { parent: "root", initial: "idle" },
    idle: { parent: "flow", on: { go: { target: "done", guard: "ready" } } },
    done: { parent: "flow", kind: "final" },
    net: { parent: "root", initial: "up" },
    up: { parent: "net", on: { drop: "down" } },
    down: { parent: "net", on: { restore: "up" } },
  },
});

const chart = new StateChart(def);
chart.activeLeaves(); // ["idle", "up"]
chart.send("drop"); // true
chart.send("go", { ready: true }); // true
chart.matches("done"); // true
```

For typed JavaScript/TypeScript authoring, `ChartBuilder` builds the same
`ChartDef` through the same validation/assembly path. It is an ergonomic API,
not a second semantics:

```js
import {
  ChartBuilder,
  StateBuilder,
  StateChart,
} from "@lazily-hub/lazily-js/statechart";

const def = new ChartBuilder()
  .state(StateBuilder.parallel("root"))
  .state(StateBuilder.compound("flow", "idle").parent("root"))
  .state(StateBuilder.atomic("idle").parent("flow").onGuarded("go", "done", "ready"))
  .state(StateBuilder.final("done").parent("flow"))
  .state(StateBuilder.compound("net", "up").parent("root"))
  .state(StateBuilder.atomic("up").parent("net").on("drop", "down"))
  .state(StateBuilder.atomic("down").parent("net").on("restore", "up"))
  .build();

const chart = new StateChart(def);
chart.send("go", { ready: false }); // false, guards fail closed
```

## Keyed collections and semantic tree

`CellMap` and `CellTree` implement the lazily-spec keyed collections layer:
value, membership, and order readers invalidate independently; stable handles
survive moves; and an atomic move bumps order without touching values.
`reconcileCollections` emits the LIS-minimized `{ insert, remove, move, update }`
operation set. `SemTree` adds a memoized ancestor-chain fold: editing one leaf
recomputes only that leaf's ancestor path, and equal folded results are
suppressed by the computed equality guard.

```js
import { CellMap, reconcileCollections } from "@lazily-hub/lazily-js/collections";
import { Context } from "@lazily-hub/lazily-js/reactive";
import { SemTree } from "@lazily-hub/lazily-js/sem-tree";

const map = CellMap.from({ order: ["a", "b"], values: { a: 1, b: 2 } });
map.moveBefore("b", "a"); // order reader invalidates; value readers do not

reconcileCollections(
  { order: ["a", "b"], values: { a: 1, b: 2 } },
  { order: ["b", "a", "c"], values: { a: 1, b: 2, c: 3 } },
).ops; // move-minimized patch

const ctx = new Context();
const rootSpec = {
  id: "root",
  value: 0,
  children: {
    order: ["leaf"],
    values: { leaf: { id: "leaf", value: 1 } },
  },
};
const tree = new SemTree(ctx, rootSpec, (value, children) =>
  value + children.reduce((sum, child) => sum + child, 0),
);

tree.value(); // 1
tree.setValue("leaf", 99); // only the ancestor chain recomputes
```

## Competing-consumer work queue

The `queue` package exports `WorkQueueCell`, a pull-based local authority with
exclusive FIFO claims, stable item IDs, fresh delivery IDs per attempt,
worker-owned ack/nack, strict visibility-timeout redelivery, bounded attempts,
and DLQ routing. Every mutation reports exact `pending_len` / `is_empty` /
`in_flight_len` / `dead_letter_len` invalidation metadata.

```js
import { WorkQueueCell } from "@lazily-hub/lazily-js/queue";

const work = new WorkQueueCell({ visibility_timeout: 30, max_deliveries: 3 });
work.push("render-report");
const delivery = work.claim("worker-a", 100).returns;
work.ack("worker-a", delivery.delivery_id);
```

The instance serializes local claims; distributed/HA assignment still requires
a leader or consensus-committed assignment log.

## CRDTs

`SeqCrdt` is the move-aware sequence CRDT: each element has independent LWW
registers for value, position, and deletion, so a move is one position
assignment rather than delete plus reinsert. `TextCrdt` is a Fugue/RGA
character CRDT: concurrent same-point inserts are preserved, deletes are sticky
tombstones, and merge is commutative / associative / idempotent. Both expose
tombstone GC behind caller-supplied causal-stability watermarks. `TextCrdt`
also satisfies the `CrdtTree` document contract: its snapshot is the delta from
an empty frontier, so full hydration and incremental exchange preserve the same
identity-bearing state.

```js
import { SeqCrdt } from "@lazily-hub/lazily-js/seq-crdt";
import { TextCrdt } from "@lazily-hub/lazily-js/text-crdt";

const seq = new SeqCrdt(1);
seq.insertBack("a", 0, 1);
seq.moveAfter("a", "b", 10);

const text = TextCrdt.fromStr(1, "hi");
const peer = text.fork(2);
peer.insert(2, "!");
text.merge(peer); // converges
```

## Durable outbox stores

The root `Outbox` class owns one append/ack/prune/replay protocol over the
five-operation `OutboxStore` boundary. `InMemoryStore` exercises that path in
tests. Browsers can open an `IndexedDbStore` from
`@lazily-hub/lazily-js/indexeddb-outbox`; await `append` before transport send
and `ackThrough` before treating an acknowledgement as committed. Reopening the
same database and channel restores the durable cursor and only unacknowledged
frames.

## IPC wire types and capability negotiation

Every IPC value round-trips the canonical externally-tagged
[`lazily-spec`][spec] JSON shape through `toWire()` / `fromWire()`.
`IpcMessage` adds `encodeJson()` / `decodeJson()`. `Snapshot` and `Delta`
represent the single-writer graph-state plane; `CrdtSync` carries the
multi-writer CRDT anti-entropy plane. `PeerPermissions` is default-deny and
filters unreadable nodes/ops out of snapshots and deltas. `SessionHandshake`
performs the fail-closed protocol/version/codec/feature check before graph
frames flow.

```js
import {
  Delta,
  DeltaOp,
  IpcMessage,
  NodeSnapshot,
  PeerPermissions,
  RemoteOp,
  Snapshot,
} from "@lazily-hub/lazily-js";

const snapshot = new Snapshot({
  epoch: 7,
  nodes: [NodeSnapshot.payload(1, "counter", new Uint8Array([42]))],
  roots: [1],
});

const wire = IpcMessage.snapshot(snapshot).encodeJson();
IpcMessage.decodeJson(wire).snapshot.epoch; // 7

const delta = Delta.next(7, [DeltaOp.cellSet(1, [43])]);
IpcMessage.delta(delta).toWire();

const permissions = new PeerPermissions();
permissions.allow(10, RemoteOp.read(1));
snapshot.filterReadable(permissions, 10).nodes.length; // 1
snapshot.filterReadable(permissions, 11).nodes.length; // 0
```

`BINDING_CAPABILITIES` advertises the JS binding truthfully: reactive core,
IPC, CRDT, keyed collections, semantic tree, sequence/text CRDT, stable-id,
state machine, state charts, permissions, capability negotiation, async context,
signaling, and the WebRTC transport are shipped; C-ABI FFI is `none` because
browser/Worker JS cannot host a native in-process ABI. The same payload types
can still be carried by any transport a host application owns.

## Cross-process zero-copy transport

`@lazily-hub/lazily-js/transport` implements the pluggable blob-backend
transport (`#lzzcpy`). A large payload is not copied through the wire codec: the
producer **spills** it to a backend (which mints a `ShmBlobRef` descriptor) and
ships only the descriptor; the receiver **routes** the descriptor by its
`backend` discriminator and **resolves** it zero-copy — reading the backend's own
bytes in place. `ShmBlobRef` gained an optional `backend` field (`shm` | `arrow`
| `in_process`) that defaults to `shm` and is omitted from the wire, so every
pre-transport descriptor round-trips byte-for-byte.

The module is **isomorphic**: it imports no FFI, so it bundles and runs in the
browser. `InProcessBackend`, `ArrowBackend`, the `BlobRouter`, and the
spill/resolve policy are pure JS and available everywhere (including a
main-thread ↔ Web Worker deployment).

```js
import {
  BlobRouter,
  InProcessBackend,
  ArrowBackend,
  spillMessage,
} from "@lazily-hub/lazily-js/transport";
import { Delta, DeltaOp, IpcMessage, IpcValue } from "@lazily-hub/lazily-js";

const backend = new InProcessBackend(); // or ArrowBackend for columnar payloads
const big = IpcValue.inline(new Uint8Array(4096));
const { message, spilledBytes } = spillMessage(
  IpcMessage.delta(new Delta({ baseEpoch: 0, epoch: 1, ops: [DeltaOp.slotValue(7, big)] })),
  backend,
); // message now carries a small SharedBlob descriptor; spilledBytes === 4096

const router = new BlobRouter().register(backend);
router.resolve(message.delta.ops[0].payload); // Uint8Array view, zero copy
```

The genuine **cross-process** `shm` backend (POSIX `shm_open` + `mmap`) is loaded
lazily and only where a runtime provides FFI — Node (via `koffi`), Bun (`bun:ffi`),
or Deno (`Deno.dlopen`, needs `--allow-ffi --unstable-ffi`). A peer process on the
same host that attaches the same name resolves the descriptor without copying
across the process boundary. In the browser (or any runtime without FFI)
`createShmBackend` rejects with `ShmUnavailableError`; guard with `shmSupported()`
and fall back to `InProcessBackend` / `ArrowBackend`.

```js
import { createShmBackend, shmSupported } from "@lazily-hub/lazily-js/transport";

if (shmSupported()) {
  const shm = await createShmBackend("my-session", { capacity: 1 << 20 });
  const ref = shm.write(new Uint8Array([1, 2, 3])); // ref.backend === "shm"
  // ...ship `ref` to a peer; the peer attaches `createShmBackend("my-session",
  // { capacity: 1 << 20, create: false })` and calls `shm.readView(ref)`.
  shm.close();
}
```

All three runtimes are verified end-to-end, including cross-runtime interop: a
Node process writes a region that a separate Deno process attaches and resolves,
proving the layout is byte-identical across FFI implementations. The `shm` region
is a bump-allocated arena with a fixed header (magic / version / capacity / epoch
/ generation / cursor) and per-entry `{ generation, epoch, len, checksum }`
validation.

## Distributed plane

The `@lazily-hub/lazily-js/signaling` and `@lazily-hub/lazily-js/distributed`
entry points ship the distributed plane. Signaling is the kebab-tagged discovery
wire protocol (`ClientMessage` / `ServerMessage`), a transport-agnostic
`SignalingRoom` that enforces the anti-spoof forwarded-`from` invariant, and a
`SignalingClient` over a pluggable socket seam. The distributed module is the
WebRTC DataChannel IPC transport (`WebRtcSink` / `WebRtcSource` with outbound
permission filtering, over any `DataChannel`) plus `CrdtPlaneRuntime`, the CRDT
anti-entropy runtime. Both are koffi-free and testable over an in-memory loopback
with zero network; "real" WebRTC is reached through a browser platform adapter
(`RtcPeerChannel` / `RtcPeerConnector`) that wraps the `RTCDataChannel` /
`RTCPeerConnection` globals with no npm dependency.

```js
import { CrdtPlaneRuntime, InMemoryDataChannel, WebRtcSink, WebRtcSource } from "@lazily-hub/lazily-js/distributed";
import { SignalingRoom, ClientMessage } from "@lazily-hub/lazily-js/signaling";
import { IpcMessage, IpcValue, PeerPermissions, OpKind, Snapshot, NodeSnapshot } from "@lazily-hub/lazily-js";

// Route a signaling handshake with server-stamped `from` (anti-spoof).
const room = new SignalingRoom();
room.receive("a", ClientMessage.join(1));
room.receive("b", ClientMessage.join(2));
room.receive("a", ClientMessage.offer(2, "SDP-A")); // -> { to: "b", message: offer{ from: 1, sdp: "SDP-A" } }

// Two replicas converge over an anti-entropy exchange.
const alice = new CrdtPlaneRuntime(1);
const bob = new CrdtPlaneRuntime(2);
alice.register(1, "doc/title");
bob.register(1, "doc/title");
const op = alice.localUpdate(1, Date.now() * 1000, IpcValue.inline([66]));
bob.ingest(alice.syncFrame(), Date.now() * 1000); // 1 op applied; re-ingest applies 0
```

## Conformance

lazily-js replays the shared `lazily-spec` fixtures for IPC, agent-doc state,
keyed collections (`CellMap`, `CellTree`, LIS reconciliation), semantic tree,
sequence and text CRDTs (incl. `TextCrdt` delta sync, `#lztextsync`:
`textcrdt_convergence.json` + `textcrdt_delta_sync.json`), manufactured text
identity, the keyed reactive maps / materialization (`#reactivemap`:
`materialization/observational_transparency.json`,
`materialization/deferral_not_deallocation.json`,
`materialization/entry_kind_orthogonal_to_mode.json` — replayed through the
single-threaded, async, and thread-safe maps), the C-ABI FFI boundary
(`schemas/ffi.json`: message codec + channel round-trip over `snapshot_*` /
`delta_*` wire), Harel state charts, the
signaling protocol (`signaling/frames.json`,
`signaling/anti_spoof_session.json`), and the distributed CRDT plane
(`distributed/crdt_sync_frames.json`, `distributed/anti_entropy_converge.json`).
It also validates generated wire values against the canonical JSON Schemas.

`npm test` builds the [`lazily-formal`][formal] Lean 4 model when that sibling
checkout and the `lake` toolchain are present. The script exits successfully
when they are absent, so npm tarball consumers and shallow clones are not forced
to install Lean; full CI verifies the proofs.

Each formal module with a JS counterpart has a matching property test that
names the Lean theorems it mirrors:

| lazily-formal module | JS test file | Mirrored theorems |
|----------------------|--------------|-------------------|
| `StateMachine` | `state-machine.test.js` | `guard_rejection_preserves_state`, `accepted_transition_advances_state`, `send_preserves_transition` |
| `StateChart` | `statechart-properties.test.js` | `enabled_empty_rejects`, `parallel_region_confluence`, `single_region_refines_flat_machine`, `single_region_enabled_at_most_one`, `recordHistory_idempotent`, `send_actions_empty_when_rejected`, `send_preserves_chart`, determinism-by-construction |
| `Reactive` | `reactive-properties.test.js` | `setCell_equal_preserves_graph`, `setCell_different_invalidates_dependents`, `recomputeSlot_equal_preserves_dependents`, `recomputeSlot_different_invalidates_dependents`, `signal_materialized_after_recompute` |
| `Collection` | `collection-properties.test.js` | `setEntryValue_preserves_{membership,order,siblings}`, `moveKey_preserves_{membership,values}`, `moveKey_advances_order`, `addKey_advances_membership_and_order`, `Family.get_idempotent_after_first` |
| `Tree` | `tree-properties.test.js` | `setNodeValue_preserves_{other_nodes,node_signals}`, `moveChild_preserves_{non_parent,parent_value}`, `moveChild_advances_order_signal_only` |
| `Materialization` | `reactive-family.test.js` | `observe_canonical`, `eager_lazy_observationally_equivalent`, `eager_materializes_all`, `lazy_defers_slots`, `materialize_present_monotone`, `lazy_present_subset_eager`, `materialize_preserves_observe`, `cell_entries_materialized_in_every_mode`, `slot_entries_deferred_under_lazy` |
| `Materialization` (thread-safe) | `thread-safe-reactive-family.test.js` | `materialize_present_comm`, `materialize_observe_comm` (materialization confluence) + the base materialization laws replayed through `ThreadSafeSlotMap` |
| `AsyncMaterialization` | `async-reactive-family.test.js` | eventual transparency (a driven async slot resolves to the canonical value; eager ≡ lazy) + present-set monotonicity through `AsyncSlotMap` |
| `ThreadSafe` | `thread-safe.test.js` | `flushBatch_empty`, `flushBatch_singleton_eq_setSource` (thread-safe batch refines `set`), `flushBatch_dependent_dirty`, `flushBatch_preserves_nondependent_dirty` |
| `Reconciliation` | `reconciliation-properties.test.js` | `lisBy_longest`, `reconcile_move_minimized`, `reconcile_stable_not_invalidated` |
| `AsyncSlotState` | `reactive-async.test.js` | `stale_completeOk_discarded`, `current_completeOk_publishes`, `current_completeErr_to_error` |
| `AsyncEffect` | `reactive-async.test.js` | `fire_blocked_during_cleanup`, `invalidate_from_idle_schedules`, `cleanupDone_resumes_deferred`, `dispose_absorbing`, `disposed_terminal` |

The thread-safe context IS mirrored on this runtime: JavaScript is
single-threaded per realm but shares memory across Web Workers via
`SharedArrayBuffer` + `Atomics`, so `ThreadSafeContext` guards every operation
with a real reentrant Atomics mutex (cross-worker mutual exclusion, degrading to
a single-realm guard where shared memory is unavailable). The `Signaling` /
`SignalingRoster` formal models are exercised through the `SignalingRoom`
fixture replay (`signaling/anti_spoof_session.json`) rather than a named-theorem
property test.

## The lazily family

| Binding | Language | Package / role |
|---------|----------|----------------|
| [`lazily-rs`][rs] | Rust | `lazily` on crates.io; single-threaded, thread-safe, and async context layers |
| [`lazily-py`][py] | Python | `lazily` on PyPI; dict-backed context plus IPC/shared-blob host types |
| **`lazily-js`** | JavaScript / TypeScript | `@lazily-hub/lazily-js`; reactive core + async context, spec wire types, state charts, CRDTs, distributed plane (signaling + WebRTC) |
| [`lazily-zig`][zig] | Zig | Zig library / FFI-oriented embedding surface |
| [`lazily-kt`][kt] | Kotlin/JVM | Kotlin reactive core plus typed state charts |
| [`lazily-dart`][dart] | Dart | Dart binding with statechart conformance |
| [`lazily-spec`][spec] | Specification | wire protocol, JSON Schemas, conformance fixtures |
| [`lazily-formal`][formal] | Lean 4 | executable formal model for the shared primitives, FSM, and state charts |

## Development

```bash
make check   # npm run build && npm test
```

- `npm run build` runs `node --check` over every shipped module.
- `npm run test:formal` builds `lazily-formal` when the sibling checkout and
  `lake` are present.
- `npm test` runs the formal check and the Node test suite.

## Benchmarks

Wall-clock benchmarks live in [`BENCHMARKS.md`](BENCHMARKS.md), with two suites
built on a zero-dependency `node:perf_hooks` harness:

- **Micro-benchmarks** ([`bench/context.bench.mjs`](bench/context.bench.mjs)) — a
  1:1 port of the single-threaded `Context` cases in lazily-rs's
  `benches/context.rs` (cached reads, cold first get, dependency fan-out,
  set-cell invalidation, computed equality suppression, effect flushing, batch
  storms, typed cache reads) so JS and Rust numbers are directly comparable.
- **Scale** ([`bench/scale.bench.mjs`](bench/scale.bench.mjs)) — a
  spreadsheet-shaped graph (`N` input cells + `N` formula slots,
  `formula[i] = input[i] + input[i - 1]`) mirroring the lazily-rs/-go/-py `scale`
  groups. At the default `N = 1,000,000` that is ~2M reactive nodes; the
  `LAZILY_SCALE_N=5000000` run covers a full 10M-cell Google Sheets workbook. A
  one-cell edit + 1,000-cell viewport read stays ~100 µs **independent of sheet
  size** — the lazy-pull property a viewport-rendered spreadsheet needs.

```bash
make bench          # micro-suite (prints a markdown table)
make bench-scale    # scale suite at N = 1,000,000
npm run benchmark-update   # refresh BENCHMARKS.md's generated micro-bench table
npm run benchmark-check    # CI gate: exit 1 if the micro-bench row set is stale
```

## Bundle size

`@lazily-hub/lazily-js` ships as pure ES modules with `"sideEffects": false` and
one file per subpath export, so bundlers tree-shake to exactly what you import.
The budgets below are enforced in CI by `npm run test:size` (size-limit); the
table is regenerated on every `npm run build` so it cannot drift from the
shipped bytes.

<!-- size-limits:start -->

Generated for package `@lazily-hub/lazily-js` version `0.27.0`. Every entry is **minified + brotlied, tree-shaken to the named import** (`size-limit` + esbuild, the same pipeline Webpack/Rollup/Vite apply via `"sideEffects": false`).

Refresh command:

```bash
npm run build            # regenerates this table as part of every build
npm run test:size        # gate: fails CI if any entry exceeds its budget
```

| Import | Size | Budget |
|---|---:|---:|
| reactive: Context | 3.50 KB ✓ | 3.60 KB |
| reactive: Context + handles + defaultEqual | 3.50 KB ✓ | 3.60 KB |
| state-machine: StateMachine | 274 B ✓ | 280 B |
| sem-tree: SemTree | 503 B ✓ | 512 B |
| stable-id: contentHash | 152 B ✓ | 152 B |
| collections: CellMap + CellTree + reconcileCollections | 1.65 KB ✓ | 1.65 KB |
| index: PROTOCOL_ID + Snapshot (tree-shaken kitchen sink) | 2.41 KB ✓ | 2.41 KB |

<!-- size-limits:end -->

## See also

- [`lazily-spec`][spec] - language-agnostic wire protocol, schemas, and
  conformance fixtures.
- [`lazily-formal`][formal] - Lean 4 formal model behind the shared behavioral
  guarantees.
- [`lazily-rs`][rs] / [`lazily-py`][py] / [`lazily-zig`][zig] /
  [`lazily-kt`][kt] / [`lazily-dart`][dart] - sibling bindings.

[rs]: https://github.com/lazily-hub/lazily-rs
[py]: https://github.com/lazily-hub/lazily-py
[zig]: https://github.com/lazily-hub/lazily-zig
[kt]: https://github.com/lazily-hub/lazily-kt
[dart]: https://github.com/lazily-hub/lazily-dart
[spec]: https://github.com/lazily-hub/lazily-spec
[formal]: https://github.com/lazily-hub/lazily-formal
[statecharts]: https://github.com/lazily-hub/lazily-spec/blob/main/docs/state-charts.md
