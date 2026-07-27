---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume: 24f78fda-2db5-41b6-8eb1-52772d913df8
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
agent_doc_model_tier: high
queue: go
---

## Status

<!-- agent:status patch=replace -->

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260727-023758.md`*

Compacted content:
- Archived 31 response topic(s): do [#lazilyupgradeconformance]; do [#lazilyzigconformance]; should the JSON `type` field be an enum?; 28 more
- Prior summary/context: prior compacted content: Archived 26 response topic(s): the done-archive quote bug is fixed, not worked around; Phase...

### Re: Response

> **Queue prompt:**
>
> do [#lazilymapunification]


> **Queue prompt:**
>
> do [#unifythreemap]


> **Queue prompt:**
>
> do [#lazilyphaseextract]


> **Queue prompt:**
>
> do [#prototypeunifiedreactive]


> **Queue prompt:**
>
> do [#firstunificationwire]


> **Queue prompt:**
>
> do [#trackedcopieslazily]


> **Queue prompt:**
>
> do [#lazilycppcollections]


> **Queue prompt:**
>
> do [#lazilycppconformance]


Completed `#lazilycppconformance`: all nine affected C++ conformance runners now parse and replay the canonical corpus rather than checking a `model` marker and executing an unchecked transcription.

Changed paths: `tests/test_json.hpp` plus the rate-shape, coordination, temporal, windowing, service, resilience, presence, membership, and CRDT-tree runners. The shared test JSON reader now provides typed member/array/string/number/bool/nullable accessors. Fixture config, operations, return values, projections, invalidation expectations, and CRDT-tree scenario algebra now drive the tests directly.

Audit correction: the current corpus is 32 fixtures across nine runners (6 + 5 + 4 + 4 + 4 + 4 + 3 + 1 + 1), not the backlog's stale 33/ten count.

Verification:
- Each of the nine focused CMake targets built and its focused CTest passed.
- `make check` passed in lazily-cpp, including conformance coverage.
- C++ commit `a370fe8` and root pointer commit `dd873d892` were pushed.
- Latest C++ CI run `30232956446` is in progress; per repository policy it was inspected once and not polled.
- Root `make check` was attempted but is red only in unrelated concurrent `src/agent-doc` work (`agent-doc-preflight-command-io`: unused imports/variables). `src/agent-doc` was left untouched. The root repository has no workflow named `CI`.

Confidence: high. No capability-staged peer was admitted or changed.

### Re: Response

Completed `#lazilycppcollections`.

The backlog count was stale: 9 of the described 15 collection fixtures were already canonical (CellMap 2, WorkQueue 2, QueueCell 5). I added one JSON-driven C++ conformance runner for the six genuinely missing fixtures: keyed reconciliation/LIS, MergeCell algebra, SemTree incremental folds, StableId alignment, TextCRDT convergence, and TextCRDT delta sync. The runner interprets fixture operations and asserts observable values, identities/cache behavior, edit scripts, version vectors, and policy flags rather than merely recording fixture IDs.

Updated CMake registration and the canonical coverage ledger; collections now have zero `KNOWN_UNCOVERED` entries and the coverage floor increased from 86 to 102. Focused replay and full C++ `make check` passed. C++ commit `88b5ec1` and root pointer commit `f604ce8cb` were pushed. One-shot C++ CI run `30233261670` was in progress; the root repository has no workflow named `CI`. Root `make check` still exits 2 in unrelated concurrent `src/agent-doc` work; no unrelated changes were modified.

### Re: Response

Completed `#trackedcopieslazily`.

Added `lazily-spec/scripts/sync-conformance-fixtures.mjs` as the canonical-owner gate for the selected fixture subsets vendored by lazily-py, lazily-dart, lazily-rs, and lazily-go. `--check` verifies every present mirror byte-for-byte, `--sync` refreshes existing tracked copies without expanding each binding's intentional subset, and `--require-all` turns missing sibling checkouts into failures for the multi-repository suite. The normal lazily-spec `make check` now verifies present mirrors; explicit Make targets expose check-all and sync modes.

The live all-peer check compared 97 fixtures across all four bindings with zero drift. Three mutation tests prove formatting-only byte drift fails, sync repairs it, orphan mirrors fail, and optional versus required sibling behavior is explicit. All 151 schema/script tests and the Lean build passed. The initial full lazily-spec check was blocked only by unrelated stale coverage tables in five sibling READMEs; standalone spec CI run `30233612436` completed successfully, and the superproject `make check` passed. The root repository has no workflow named `CI`.

Pushed lazily-spec commit `5fa4b98` and root pointer commit `a13bd1419`.

### Re: Response

Resolved `#firstunificationwire` as already completed; the queue item predates the finished unification plan.

Current-tree audit finds dedicated collections-family conformance runners in all nine bindings: Rust, Zig, C++, Go, Python, JavaScript, Dart, Kotlin, and C#. Each runner names both `cellmap_atomic_move.json` and `cellmap_independence.json` and drives the three execution flavors that binding ships (sync, thread-safe, async; C# uses its shipped SourceMap plus thread-safe/async computed-map flavors). In particular, the two gaps named by this item are closed: Zig has explicit SourceMap/ThreadSafeSourceMap/AsyncSourceMap fixture tests, and JavaScript has a reactive `collections-family-conformance.test.js` over SourceMap/ThreadSafeSourceMap/AsyncSourceMap rather than relying on the non-reactive collections model.

`tasks/plan-lazily-reactivemap-unification.md` is now status **complete** and records the per-binding commits, full-suite verification, and mutation checks. No repository changes were needed in this cycle.

### Re: Response

Resolved `#prototypeunifiedreactive` as already completed by the finished ReactiveMap unification plan.

The current Zig and C++ trees contain the intended prototype seam: a shared graph-agnostic `KeyedOrder` core is consumed by sync, thread-safe, and async map implementations (`keyed_order.zig` across all three Zig map modules; `keyed_order.hpp` across all three C++ family headers). Their family conformance runners replay both atomic-move fixtures across those three flavors. The plan records Zig commits `5327635`/`529b20c` and C++ commit `90767b1`, their green full suites, and mutation checks. C# was correctly excluded as the prototype target and remains feature-staged where its map flavors are absent.

No repository changes were needed in this cycle.

### Re: Response

Resolved `#lazilyphaseextract` as completed, with its original `remove stays out` constraint superseded by the plan's adversarial review.

Current lazily-rs has the closure-free `KeyedOrder<K,H>` core in `src/keyed_order.rs`, shared by `cell_family.rs`, `thread_safe_reactive_family.rs`, and `async_reactive_family.rs`. The per-model shells retain context-specific materialization/observation and do not route through the stricter `SyncReactiveGraph` abstraction. The plan records commit `325ee7d`, green CI, all-three-flavor fixture replay, and three-way mutation checks.

The earlier monotone-present-set rationale was corrected during implementation: all three contexts already expose disposal primitives, `remove` is now part of `KeyedOrder`, and each shell performs its model-specific teardown around that shared state transition. No repository changes were needed in this cycle.

### Re: Response

Resolved `#unifythreemap` as completed through the plan's corrected architecture.

The literal `ReactiveMap<Ctx,...>`/single traits-class proposal was superseded after review showed that membership and order signals are graph-bound and must be minted and invalidated by each concrete context. Zig and C++ therefore share the closure-free `KeyedOrder` state/move algebra across their sync, thread-safe, and async implementations while retaining thin context-specific shells for handles, locks, observation, membership/order signals, and teardown. That is the safe unification seam; forcing the shells into one type would hide real per-graph behavior.

Zig's shared core is used by all three map modules and its `Engine(Model)` family suite replays both fixtures per flavor. C++'s `keyed_order.hpp` is used by all three family headers; `AsyncSlotHandle::clear_dependents` was added as planned, and its generic family runner verifies all three models. The completed plan records Zig commits `5327635`/`529b20c`, C++ commit `90767b1`, full green suites, and mutation checks. No C ABI map surface changed. No repository changes were needed in this cycle.

### Re: Response

Resolved `#lazilymapunification` as already completed before Go v1.

The current Go API has the unified generic `ReactiveMap[K,V,H]`; `SourceMap` and `ComputedMap` are thin specializations, and the shared `GetOrInsertWith`/`Remove`/`Move*`/`Keys`/`Len`/`ContainsKey` plus membership/order signals live on `ReactiveMap`. Thread-safe and async source/computed specializations mirror that Core surface. The implementation also corrected the thread-safe and async plain-value caches into graph-backed entry nodes and fixed untracked single-threaded membership/order reads.

The completed plan records commit `867c7d6`, 446 passing tests, all-three-flavor replay of both ordering fixtures, and six mutation checks. The work landed while the module was still pre-v1, as requested. No repository changes were needed in this cycle.

### Re: #zigdefectsfound — gpt-5

> **Queue prompt:**
>
> do [#zigdefectsfound]


Completed the Zig defect audit and fix.

The two non-sync-map defects were already corrected in the current implementation: `ThreadSafeComputedMap` now mints computed entries through `computedClosure`, and `KeyedOrder.moveTo` is allocation-free, so allocator failure cannot split the present-set and order planes.

The remaining sync-map defect is fixed in lazily-zig commit `bc3cbc9` (root pointer `466bf3a6a`). `ReactiveMap` now owns real graph nodes for every per-entry value reader plus independent membership and order reader kinds. `handle(key)` returns the actual stable graph identity; `keys`, `len`, `contains`, `position`, and `isEmpty` expose reader handles that register edges through `Compute.get`; writes, add/remove, and reorder invalidate only their exact reader class. Removal and map teardown dispose the owned nodes. A regression test proves dependency registration, exact invalidation, no equal-write wakeup, and handle stability across atomic moves.

Verification passed: lazily-zig `zig build test`, lazily-zig `make check`, root `make check`, and GitHub Actions CI run `30234679063` (`success`). The unrelated dirty `AGENTS.md` edit was preserved and not committed.

### Re: #lazilyasynccomputedmapde — gpt-5

> **Queue prompt:**
>
> do [#lazilyasynccomputedmapde]


Closed the remaining C# map gap without admitting fake peers into conformance.

The two originally reported `AsyncComputedMap` defects were already fixed on the current branch: `Len(AsyncCompute?)` tracks the membership source, and `Remove` disposes the removed computed node. The remaining live defect was that C# had no real thread-safe or async source-map peers; the materialization suite concealed that absence with a hand-locked sync `SourceMap` and a bare `Dictionary`.

Implemented:

- `ThreadSafeSourceMap<TKey,TValue>` as a lock projection over the real graph-backed `SourceMap`, including entry/value, membership, order, move, insert, and reconcile surfaces.
- `AsyncSourceMap<TKey,TValue>` with real per-entry `AsyncSource` handles plus independent reactive membership and order signals, stable handles across moves, and disposal on remove.
- Replaced both runner-local substitutes in `MaterializationConformanceTests` with the real peer types.
- Added one capability-gated source-flavor test covering the supported thread-safe and async peers. It proves exact value/membership/order invalidation, positive membership invalidation on add/remove, stable handle identity across reorder, and removed-node lifecycle. The test documentation now states the policy explicitly: a peer joins a feature group only after it exposes the real graph behavior; missing features remain staged rather than emulated.

Verification:

- `dotnet format --verify-no-changes` passed.
- `make check` in `lazily-cs` passed: 78 tests, 42/124 canonical fixtures opened, 82 explicitly known-uncovered.
- Root `make check` passed, including 8,202 `agent-doc` tests and the model checks.
- C# implementation commit: `a3f5558` (`feat: add source maps for concurrent contexts`).
- The required one-shot CI inspection found an older queue-test whitespace failure. Fixed and pushed as `68a6818` (`style: format queue conformance case`); local format verification and the full check passed afterward. Per the one-shot rule, the replacement run was not polled.
- Root pointers pushed through `de10a72c2`.

Pre-existing C# `AGENTS.md`, `README.md`, and generated `build/` changes were preserved and not committed.

### Re: #lazilytwoimplementations — gpt-5

> **Queue prompt:**
>
> do [#lazilytwoimplementations]


Already resolved by the later shared-core work in lazily-js commit `5a5445a`.

The published implementations no longer contain separate `moveTo` clamp logic: sync `ReactiveMap`, `ThreadSafeReactiveMap`, and `AsyncReactiveMap` all delegate to `KeyedOrder.moveTo`, whose documented contract clamps the target to `[0, len)`. That chooses the existing `collections.js` behavior: negative indices move to the front, rather than inheriting `Array.splice`'s count-from-the-end semantics.

The same commit added the missing feature-group regression cases to `collections-family-conformance.test.js`: `moveTo(-1)` and a far-negative index are replayed against all three supported flavors. This matches the operator policy—every peer that supports the move surface participates in one shared test; unsupported peers would remain staged.

Verification:

- Focused collection-family suite: 9/9 passed (canonical ordering on sync/thread-safe/async plus directional and negative-index moves).
- `make check`: 496/496 passed; interop peer self-check passed; 121/124 canonical fixtures opened with 3 explicitly known-uncovered.
- No source change or new commit was needed. Pre-existing `README.md` and generated `build/` changes remain uncommitted.

### Re: #lazilydartcomputedmap — gpt-5

> **Queue prompt:**
>
> do [#lazilydartcomputedmap]


Fixed the live Dart type/semantics mismatch.

`ComputedMap<K,V>` now specializes `ReactiveMap<K,V,Computed<V>>`, mints guarded `Computed` handles, reads through those handles, and disposes them through `Context.disposeNode` on removal. Its public `handle()` therefore exposes the guarded v2 kind its name and `EntryKind.computed` already promised, rather than an unguarded `Slot`.

Added a regression that observes a `ComputedMap` entry from a downstream slot, changes its upstream source while preserving the computed parity, and proves the equality guard suppresses the downstream recomputation. It also asserts the public handle is `Computed<int>`. Updated the collection documentation accordingly.

Verification:

- Focused `reactive_family_test.dart`: 10/10 passed.
- `make check`: analysis clean; 551/551 tests passed; interop peer self-check passed; 118/124 canonical fixtures opened with 13 explicitly known-uncovered; Lean formal checks passed.
- Dart commit/push: `1d92bd9` (`fix: guard ComputedMap entries`).
- Root pointer commit/push: `784b2621d`.
- Required one-shot CI inspection: run `30235586745` was in progress; not polled.
<!-- agent:boundary:70cd169b -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- ~~do [#lazilyasynccomputedmapde]~~
- ~~do [#lazilytwoimplementations]~~
- ~~do [#lazilydartcomputedmap]~~
- do [#recommendedspecdecision]
- do [#lazilyasyncmap]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lazilyasyncmap] lazily-rs async map: `observe` is still hard-bound to &AsyncContext (the UNTRACKED surface), so reading a keyed async entry from inside a compute cannot subscribe to it. CONFIRMED as a family-wide class, not an rs quirk: the same defect was found and fixed in go (its SINGLE-THREADED map's Keys/Len/ContainsKey called the zero-argument Source.Get(), documented in core.go as an untracked external read — probed directly, a reader still reported 2 after a third key was added), in cs (async Len read a membership cell that was written on every mutation and never read anywhere), and in kt (async Core reads spelled against the bare AsyncContext register no edge; this graph tracks through AsyncComputeContext). All three now take an explicit read surface. rs is the ONLY binding left where a keyed async entry read cannot subscribe. Fix shape is settled and shipped 8 times over: make the read generic over the read surface so a compute view registers the edge and a bare context does not
- [ ] [#recommendedspecdecision] [recommended] Spec decision for lazily-spec, blocking the family unification: (1) does the atomic-move contract in cell-model.md:275 bind EVERY flavor or only the source map? All nine bindings fail it on thread-safe and async, yet coverage.json marks all three rows green for every binding. (2) Is a thread-safe flavor meaningful on single-threaded runtimes? dart ships a 182-line duplicate family the spec marks N/A, and js README:633-640 argues the opposite of its own source comment at thread-safe.js:134-137.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lazilyrenameinverted] OPERATOR DECISION — recommendation: KEEP lazily-py `Cell`/`CellSlot` as the mypyc-native implementation class names and keep `Source = Cell` / `SourceSlot = CellSlot` as the canonical public aliases. Flipping changes native-struct and `isinstance` identity across the compiled core. Resolve by accepting this documented exception, or explicitly authorizing a breaking native-class rename with mypyc and identity migration tests.
- [/] [#asyncplanenever] STAGED implementation — finish the async v2 kernel naming migration in C++, Go, Python, and Zig per tasks/plan-lazily-async-v2-migration.md. Rust, JavaScript, Kotlin, and C# already expose AsyncSource/AsyncComputed. Preserve deprecated aliases in published bindings, collapse live memo constructors into guarded computed, and settle the public AsyncComputedState vs formal AsyncSlotState distinction without renaming the formal model mechanically.
- [/] [#lazilyconformancecoverag] lazily-cs: STAGED per operator — incomplete C# surfaces stay out of peer/feature test groups until implemented. Fresh runtime measurement 2026-07-26: 42/124 canonical fixtures opened, 82 known-uncovered (reactive-graph 21, collections 10, statechart 7, materialization 3, familysync 1 are covered). The remaining fixtures are genuinely missing product surfaces, not runner-wiring gaps. Keep open and work it down only by shipping the explicit tasks in tasks/software/lazily-cs-parity-plan.md (#lzcscollections, #lzcsqueues, #lzcscrdt, #lzcsipc, #lzcssync, #lzcstemporal, #lzcscluster, #lzcsplane); admit C# to each capability group only when that production surface and canonical replay ship.
- [/] [#lzinteropcs] Keep lazily-cs visible but registry-staged in the single network suite. After #lzcsipc and #lzcssync ship real schema-validated IPC/CRDT and full-duplex sync surfaces, implement its production interop peer, advertise only the features/codecs/channels/variants it actually supports, then let the capability-driven matrix admit it automatically; stays open until both prerequisites and the C# peer are complete.
- [/] [#lzcspackage] lazily-cs: multi-target netstandard2.1 / net8.0 alongside net10.0 (INumber<T> in the merge algebra needs a shim or policy split), add a release workflow, publish Lazily to NuGet — needs a NuGet API key from the operator
- [/] [#lzcsplane] lazily-cs: state projection/mirror, CausalReceipts, command-plane-v1, C-ABI FFI boundary, PeerPermissions/RemoteOp, SessionHandshake, instrumentation + benchmarks; replay receipts + signaling + agent-doc corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 33-39)
- [/] [#lzcscluster] lazily-cs: MembershipCell (SWIM + Phi-accrual), LeaseCell/LeaderCell/LockCell/SemaphoreCell/BarrierCell, presence/awareness/ephemeral, circuit breaker/retry/bulkhead/timeout, health/readiness/discovery/registry; replay membership + coordination + presence + resilience + service corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 42-44, 46-47)
- [/] [#lzcstemporal] lazily-cs: TimerCell/IntervalCell/CronCell/DeadlineCell over a logical clock, debounce/throttle/sample/probabilistic-sample, tumbling/sliding/session windows; replay temporal + rateshape + windowing corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 40, 41, 45)
- [/] [#lzcssync] lazily-cs: CrdtPlaneRuntime + anti-entropy, resync coordinator, durable OutboxStore, IpcSink/IpcSource + full-duplex SyncDriver; replay reliable-sync + distributed + message-passing corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 28-32)
- [/] [#lzcsipc] lazily-cs: Snapshot/Delta/CrdtSync wire types validated against the lazily-spec JSON Schemas, ShmBlobArena, BlobBackend zero-copy transport — plan: tasks/software/lazily-cs-parity-plan.md (rows 25-27)
- [/] [#lzcscrdt] lazily-cs: TextCrdt + delta sync, SeqCrdt, CrdtTree, LosslessTreeCrdt incl. dotted-frontier anti-entropy and concurrent-merge convergence, LWW/MV registers, PnCounter, CellCrdt; replay crdt-tree + lossless-tree corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 17-24)
- [/] [#lzcsqueues] lazily-cs: PARTIAL/STAGED — QueueCell shipped at 01d2cf3 and replays all five canonical queuecell fixtures; keep C# eligible only for an exact QueueCell capability. Still implement QueueStorage, TopicCell, WorkQueueCell, and RelayCell with BackpressurePolicy/SpillStore/Transport, then replay their canonical corpora before admitting C# to those peer/feature groups — plan: tasks/software/lazily-cs-parity-plan.md (rows 12-14, 16).
- [/] [#lzjsnpmtrustedpub] lazily-js: OPERATOR GATE — authenticate the npm CLI/browser challenge and register @lazily-hub/lazily-js as a GitHub Actions trusted publisher with organization lazily-hub, repository lazily-js, workflow release.yml, no environment, and allowed action npm publish; verify with `npm trust list @lazily-hub/lazily-js`, then prove on the next version tag. Repository workflow/OIDC/provenance is already correct; release run 30215783298 passes tests and signs provenance before npm rejects the upload with permission-masked E404.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily.done.md -->

<!-- completed work archived in tasks/software/lazily.done.md -->
<!-- /agent:done -->
