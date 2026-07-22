# Plan: lazily-rs realtime + distributed primitives (swiss-army-knife expansion)

**Goal:** ship the reactive primitives that turn lazily from "data-convergent
reactive graph + transport" into a complete swiss-army knife for realtime +
distributed apps where an instance is simultaneously **a peer/replica in a
network** and **a host of embedded services**.

**Owner:** lazily-rs (`src/lazily-rs`).
**Session:** `tasks/software/lazily-rs.md`.

**Companion plan:**
[`plan-lazily-py-native-backend.md`](plan-lazily-py-native-backend.md) (`#lzpybackend`)
gives lazily-py an optional lazily-cpp native backend (faster hot paths + non-GIL
multithreading). It consumes the primitives designed here; the
**backend-portability rules** below (payload class + compute-core split +
backend-blind conformance) exist so each new primitive lands in lazily-py with a
C++-eligible compute core that backend can accelerate. Honor those rules per
primitive and the backend work stays mechanical.

## Design recap — what's already solid

Already shipped (do not redo):

- Reactive graph — `Context` / `SlotHandle` / `CellHandle` / `SignalHandle` /
  `EffectHandle`, `ReactiveMap` / `CellMap` / `SlotMap` (`#reactivemap`), `CellTree`,
  `SemTree`, `ThreadSafeContext`, `AsyncContext`.
- Merge algebra — `MergePolicy` (`KeepLatest`/`Sum`/`Max`/`SetUnion`/`RawFifo`),
  `MergeCellHandle<T,M>`, `Reactive<T>` / `Source<T>` (`merge.rs`).
- Conflating relay — `RelayCell<T,M>` + `BackpressurePolicy` + `Overflow`
  (Block/DropNewest/DropOldest/**Conflate**/Spill) (`relay.rs`).
- Relay policies — `RatePolicy` (token bucket), `WindowPolicy` (count + tick
  flush = debounce/throttle), `ExpiryPolicy` (TTL), `PriorityStorage`, `KeyedRelay`
  (`relay_policy.rs`).
- Queues — `QueueCell` (SPSC), `WorkQueueCell` (competing consumer), `TopicCell`
  (pub/sub).
- CRDTs — `LwwRegister` / `MvRegister` / `PnCounter` / `SeqCrdt` / `TextCrdt` /
  `CrdtTree` / `LosslessTreeCrdt` / `OpLog` / `VersionVector`.
- Transport — IPC / `ShmBlobArena` / `BlobBackend` (InProcess/Arrow/SHM) /
  WebRTC (str0m) / WS.
- Durable — `DurableOutbox`, `SqliteOutbox`, `SpillStore`.
- Command plane — `CommandMessage` / `CommandPolicy` / `CommandProjection` /
  `CommandTransport` + `Receipt*`.
- State — `StateMachine`, `StateChart`, reconcile, `stable_id`.

## What's missing (the plan's scope)

Four families + one structural refactor. Every phase is dependency-ordered and
ends with `make check` + spec-test-build-install-commit-push.

### Phase 0 — `#lzrateshape`: lift coalescence into standalone source primitives

**Answer to operator question:** debounce / throttle / time-sampling already
exist algorithmically inside the relay plane — they are trapped behind
`RelayCell::egress`. This phase extracts them so any `Reactive<T>` source can be
rate-shaped, not just a relay.

- `DebounceCell<T>` — wraps any `Reactive<T>`; emits the latest value after a
  quiet period. Implementation = `KeepLatest` `MergeCellHandle` + lifted
  `WindowPolicy::tick` (no new algorithm).
- `ThrottleCell<T>` — leading-edge (first-in-window passes, rest
  `DropNewest`) and trailing-edge variants; wraps `Reactive<T>`.
- `SampleCell<T>` — deterministic every-Nth (count) and every-T (time)
  sampling; wraps `Reactive<T>`.
- `ProbabilisticSampleCell<T>` — tail-sampling operator (rate `f64`,
  injectable `Rng`); NEW algorithm (not in `RatePolicy`). Use case: trace
  sampling, hot-path observability.
- **Refactor:** move `WindowPolicy` / `ExpiryPolicy` / `RatePolicy` out of
  `relay_policy.rs` into a new `rateshape.rs`; relay policies re-export them.
  Relay semantics unchanged (regression: `relay_policy.rs` tests must pass
  unmodified).

Tests: debounce holds the latest-then-emits; throttle leading/trailing edge;
count/time sampling determinism; probabilistic sampling distribution property
test; relay policy parity post-refactor.

### Phase 1 — `#lztime`: temporal source primitives

The foundation everything temporal (leases, expirations, retries, windows) sits
on. Time modeled by the same **logical clock** discipline already used in
`relay_policy.rs` (`tick`/`advance`), with platform bindings driving it.

- `TimerCell` — single-shot source: `None → Some(())` at tick `t`. The
  primitive that every `ExpiryPolicy`/deadline retry wants to read.
- `IntervalCell` — periodic source; emits every `n` ticks.
- `CronCell` — cron-expression-shaped `IntervalCell` (next-fire = next tick
  matching the pattern).
- `DeadlineCell<T>` — pairs a value with a deadline; flips to
  `Expired(T)` at the deadline. Composite over `TimerCell` + `Cell<T>`.

Trait work: a `ClockSource` / `TimelineSource` trait so `TimerCell`,
`IntervalCell`, `CronCell` share a `tick(&mut self, now: u64)` driver and any
runtime (tokio, async-std, manual game loop) can drive them.

Tests: deterministic under logical clock; PartialEq propagation only on actual
fire; idempotent fire; integration with `tokio::time` via a binding adapter.

### Phase 2 — `#lzmemb`: membership + failure detection

The load-bearing primitive for any peer topology. Every coordination primitive
below reads this.

- `MembershipCell` — reactive view of the live peer set, keyed by `PeerId`.
  Backed by SWIM-style gossip + Phi-accrual failure detector so join / leave /
  suspect are reactive signals. State per peer: `Alive | Suspect | Dead | Left`.
- `PeerSet` — derived `Reactive<BTreeSet<PeerId>>` over `MembershipCell` (only
  `Alive` peers).
- `PeerChangeEvent` — diff stream (joined / left / state-change) over the
  membership cell.

Reuse `NodeId` / `PeerId` / `PeerPermissions` from `distributed.rs`. The
membership layer lives *below* the CRDT plane — it tells the CRDT plane which
peers are valid sync targets. Wire to existing `RelayTransport` /
`Transport` (`relay_transport.rs`) so SWIM packets ride the same transport as
deltas.

Tests: Phi-accrual bounds under synthetic clocks; join/leave propagation;
suspect→dead timeout; reactive invalidation only on set change (PartialEq
guard).

### Phase 3 — `#lzcoord`: distributed coordination

Reads `MembershipCell` + `LeaseCell` from `#lztime`/`#lzmemb`.

- `LeaseCell` — a reactive lease grant (`holder: Option<PeerId>`, `expiry: u64`),
  renewed by a heartbeat, expired by `DeadlineCell`. Single-writer authority
  for a scope.
- `LeaderCell` — wraps `LeaseCell` + `StateMachine` (leader / follower /
  candidate); `current_leader()` invalidates on re-election.
- `LockCell` — distributed mutex over `LeaseCell` + fencing token
  (`u64`-monotone). Blocks via reactive `is_full`-style reader (same pattern
  as `RelayCell`).
- `SemaphoreCell<N>` — bounded permit pool; reactive `permits_available`.
- `BarrierCell` / `QuorumCell` — wait-for-N reactive gate (write barrier,
  config rollout, migration coordinator).

Compose with existing single-writer-effect authority in the CRDT plane; a
leader's writes already have authority via `NodeId`. Fencing tokens prevent
stale-leader writes — wire into `CommandPolicy` / `Receipt*` for fence checks.

Tests: leader handover under lease expiry; fencing rejects stale leader; lock
release on membership loss; quorum gate opens at N/2+1.

### Phase 4 — `#lzpresence`: presence + ephemeral plane

The CRDT plane is durable. Collaborative/realtime apps also need an
**ephemeral** plane that does *not* persist.

- `PresenceCell<K, V>` — per-peer ephemeral state keyed by `PeerId` (online,
  device, capabilities); auto-evicts on `MembershipCell` Dead/Left. Reuses
  `ExpiryPolicy` (lifted in Phase 0) for heart-beat-driven liveness.
- `AwarenessCell<T>` — typed ephemeral broadcast (cursors, selections, typing
  indicators) layered on `TopicCell` with a TTL. Different shape from CRDT
  because values don't merge — last-writer per peer.
- `EphemeralCell<T>` — single-value cell with auto-expiry (composite over
  `ExpiryPolicy` + `Cell<T>`). "Last value seen in window N."

This is the most user-visible realtime win; agent-doc itself wants
`AwarenessCell` for live cursors.

Tests: presence auto-evicts on membership loss; awareness last-writer-per-peer;
ephemeral expiry clears value; ephemeral plane never writes to durable outbox.

### Phase 5 — `#lzwindow`: stream windowing

Lift `WindowPolicy` into a real stream-window primitive over `TopicCell` /
`RelayCell`.

- `WindowedCell<T, A>` — tumbling / sliding / session windows over a stream,
  emitting window aggregates. `A: MergePolicy<T>` is the aggregate (Sum, Max,
  SetUnion, custom).
- `SessionWindow` — gap-based sessionization (new session on idle gap >
  threshold); threshold is a reactive cell (adaptive).
- `TumblingWindow(n)` — fixed-size count or time windows.
- `SlidingWindow(size, slide)` — overlapping windows.

Reuse `MergePolicy` algebra — window aggregation IS a merge, so `Sum`/`Max`/
`SetUnion` windows compose without new code.

Tests: tumbling count/time boundaries; sliding overlap correctness; session
gap detection; aggregate = windowed merge (associativity holds).

### Phase 6 — `#lzresilience`: fault-tolerance primitives

Compose with `CommandTransport` / `CommandPolicy` so RPCs degrade gracefully.

- `CircuitBreakerCell` — reactive state machine (Closed / Open / HalfOpen)
  driven by success/failure rate over a sliding window (`WindowedCell` from
  Phase 5). Gates `CommandTransport::send` — closed circuit = passthrough,
  open = fast-fail.
- `RetryPolicyCell` — exponential-backoff source (decorrelated jitter) feeding
  a derived slot; pairs with `WorkQueueCell` redelivery semantics.
- `BulkheadCell<N>` — bounded permit pool isolating a downstream (per-tenant,
  per-shard). Reactive `permits_in_use` for observability.
- `TimeoutCell<F>` — wraps a future/call with a `DeadlineCell`; cancels or
  fast-fails on expiry.

Tests: breaker trips on error rate, half-open probes, resets on success;
backoff distribution property; bulkhead rejects over-capacity; timeout fires
under logical clock.

### Phase 7 — `#lzservice`: embedded-service plane

The story for "an instance is also a host of services." Composes everything
above.

- `HealthCell` — composed probe (deps + downstream + lease state) → reactive
  health for `/health`.
- `ReadinessCell` — composed probe (deps ready + leader known + lease valid)
  → reactive readiness for `/ready`.
- `DiscoveryCell` — reactive map `service → endpoint` fed by
  `MembershipCell` + service registration. "Call service X" → "call current
  leader for shard Y."
- `ServiceRegistry` — durable registration table (uses `DurableOutbox` pattern)
  + reactive projection.

Tests: health/readiness degrade correctly on dependency failure; discovery
updates on leader handover; registry survives restart.

## Cross-cutting structural work

- **Phase 0 refactor:** lift `WindowPolicy` / `ExpiryPolicy` / `RatePolicy`
  out of `relay_policy.rs` into `rateshape.rs`. Relay policies re-export.
  Same algorithms, broader audience.
- **`ClockSource` trait** (Phase 1): shared driver interface for temporal
  sources; one binding per runtime (tokio / async-std / manual).
- **Ephemeral plane is distinct from durable plane.** CRDT plane writes
  through `Outbox`/`SpillStore`; ephemeral plane (Phase 4) must NOT.
  Encode as a trait / marker (`Ephemeral` vs `Durable`) so the durable
  outbox can statically reject ephemeral ops.

### Backend-portability rules (so lazily-py's native backend lands cleanly)

These three rules are load-bearing for the lazily-cpp backend defined in
[`plan-lazily-py-native-backend.md`](plan-lazily-py-native-backend.md). They cost
almost nothing when designing each primitive here but are expensive to retrofit.

- **Declare a payload class per primitive.** Every new primitive states whether
  its value is a **`BytesPayload`** (canonically serializable — CRDT ops, IPC /
  membership / gossip frames, outbox records) or a **`PyObjectPayload`** (an
  opaque live language value — `Slot[object]`, an aggregate holding user
  objects). This routes the primitive to the correct backend layer:
  `BytesPayload` → flat C-ABI, GIL-releasable; `PyObjectPayload` → nanobind
  handle path. A payload-polymorphic primitive must expose a **bytes-erased
  core** so the C++ backend needs *one* monomorphization, not one-per-`T`.
- **Split compute core from reactive glue.** Design each primitive so its hot
  inner loop is a pure, side-effect-free function over plain state — the part
  worth pushing to C++ — separated from the thin reactive-signal emission.
  Examples: `MembershipCell` = Phi-accrual math + SWIM state machine (pure,
  C++-eligible) vs. signal emission (thin); `WindowedCell` = the aggregate fold
  (already `MergePolicy`, bytes-eligible) vs. window bookkeeping;
  `CircuitBreakerCell` = the rate/threshold state machine vs. the reactive gate.
  This "compute core is pure and erasable" rule is what lets the C++ backend
  accelerate a primitive *without* re-implementing its Python ergonomics.
- **Conformance is backend-blind.** The `lazily-spec/conformance` fixture family
  each primitive ships (see Constraints) must be run against **both** the
  pure-Python and native backends. A primitive is only "ported to the C++
  backend" once it passes the identical fixtures under `LAZILY_BACKEND=native`.

## Constraints / non-goals

- **No new transport.** SWIM membership (Phase 2) and presence (Phase 4)
  packets ride existing `RelayTransport` / `Transport` (`relay_transport.rs`).
- **No new merge algebra.** Window aggregation (Phase 5) and breaker windows
  reuse `MergePolicy`. New `ProbabilisticSampleCell` is the only new
  algorithmic piece.
- **Each phase is plan-first where it adds public API** (Phase 3 coordination
  primitives, Phase 4 presence plane), `cargo fmt` +
  `clippy --all-features -D warnings` + tests + `make check`, then
  `#spec-test-build-install-commit-push`.
- **Ports:** new primitives land in lazily-rs first; lazily-py / lazily-zig
  mirroring is filed separately (same pattern as
  `plan-lazily-state-machine-mirroring.md`). lazily-kt / lazily-js consume
  projections only. Each new primitive must obey the **backend-portability
  rules** above (declare a payload class, split compute core from glue) so its
  lazily-py mirror carries a C++-eligible core for the native backend — not just
  a pure fallback. The backend itself is designed in
  [`plan-lazily-py-native-backend.md`](plan-lazily-py-native-backend.md).
- **Spec:** each new primitive gets a `lazily-spec/conformance` fixture family
  + a Lean lemma in `lazily-formal` where there is an invariant worth proving
  (lease monotonicity, fencing-token monotonicity, breaker state machine,
  window aggregate associativity).

## Suggested sequence (incremental ship)

1. `#lztime` (Phase 1) — unblocks leases, expirations, windows.
2. `#lzrateshape` (Phase 0) — small, high-leverage (debounce/throttle on any
   slot). Do before Phase 5 because Phase 5 builds on the lifted primitives.
3. `#lzmemb` (Phase 2) — unblocks everything distributed beyond data
   convergence.
4. `#lzcoord` (Phase 3) — leader/lease/lock.
5. `#lzpresence` (Phase 4) — most user-visible realtime win; agent-doc wants
   this.
6. `#lzwindow` (Phase 5) — stream windows.
7. `#lzresilience` (Phase 6) — circuit breaker / retry / bulkhead.
8. `#lzservice` (Phase 7) — the embedded-service story.

Phases 4–7 can be parallelized across separate review tracks once their
prerequisites (1, 2, 0) land; the dependency DAG is:
`lztime → lzrateshape → lzwindow`, `lztime → lzmemb → lzcoord`,
`lzmemb → lzpresence`, `lzwindow + lzcoord → lzresilience → lzservice`.

### Native backend track (runs alongside this plan)

The lazily-py native backend (`#lzpybackend`,
[`plan-lazily-py-native-backend.md`](plan-lazily-py-native-backend.md)) is
**independent of this plan's critical path** and can start immediately — its
early phases (`C0`/`C1`) accelerate what lazily-py already ships. Its `C3` phase
interleaves here: each primitive's compute core is pushed to the backend as that
primitive is mirrored to lazily-py. See that plan for the full C0–C4 sequence.
