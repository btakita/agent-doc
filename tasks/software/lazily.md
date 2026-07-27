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
<!-- agent:boundary:0111a1d3 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- ~~do [#lazilycppconformance]~~
- ~~do [#lazilycppcollections]~~
- ~~do [#trackedcopieslazily]~~
- ~~do [#firstunificationwire]~~
- ~~do [#prototypeunifiedreactive]~~
- do [#lazilyphaseextract]
- do [#unifythreemap]
- do [#lazilymapunification]
- do [#zigdefectsfound]
- do [#lazilyasynccomputedmapde]
- do [#lazilytwoimplementations]
- do [#lazilydartcomputedmap]
- do [#recommendedspecdecision]
- do [#lazilyasyncmap]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lazilyasyncmap] lazily-rs async map: `observe` is still hard-bound to &AsyncContext (the UNTRACKED surface), so reading a keyed async entry from inside a compute cannot subscribe to it. CONFIRMED as a family-wide class, not an rs quirk: the same defect was found and fixed in go (its SINGLE-THREADED map's Keys/Len/ContainsKey called the zero-argument Source.Get(), documented in core.go as an untracked external read — probed directly, a reader still reported 2 after a third key was added), in cs (async Len read a membership cell that was written on every mutation and never read anywhere), and in kt (async Core reads spelled against the bare AsyncContext register no edge; this graph tracks through AsyncComputeContext). All three now take an explicit read surface. rs is the ONLY binding left where a keyed async entry read cannot subscribe. Fix shape is settled and shipped 8 times over: make the read generic over the read surface so a compute view registers the edge and a bare context does not
- [ ] [#lazilyphaseextract] lazily-rs Phase A: extract the closure-free KeyedOrder<K,H> core (entries + order + membership + present-set + position/keys/len/contains_key + move_to/move_before/move_after) shared by all three families; thin per-model shells keep materialize/observe. Do NOT route through SyncReactiveGraph — its Send+Sync bound is stricter than the concrete Context API and would break Rc-capturing callers. `remove` stays out of the core (monotone present set is fixture- and Lean-backed). Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#unifythreemap] Unify all three map families in lazily-zig and lazily-cpp — both can fold async in, unlike the rest. zig: fn ReactiveMap(comptime Ctx,K,V,kind) with Ctx.Handle/Ctx.Observed/Ctx.Lock projections; the proof already exists in-tree as Engine(comptime Model) at reactive_graph_conformance.zig:1206 over SyncModel/TsModel/AsyncModel. cpp: traits class + existing duck-typed Cx param; only Source::clear_dependents(Context&) and Computed::clear(Context&) block it, and AsyncSlotHandle needs one added clear_dependents. Neither exposes maps on its C ABI. Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#lazilymapunification] lazily-go map unification BEFORE it hits v1 — still v0.22.0 on an unversioned module path, so a breaking change costs nothing now; after v1 it costs a /v2 module path plus every import rewrite. Mechanism: closure fields (already proven in-repo at reactive_map.go:92-96) or one extra type param. Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#zigdefectsfound] zig defects found while surveying, independent of unification: (1) the sync ReactiveMap is NOT graph-backed — stores plain V and fakes reactivity with u64 counters callers must poll, so keys()/len()/contains() register no edge (reactive_map.zig:140-419 touches ctx only for .allocator); (2) ThreadSafeComputedMap mints the WRONG node kind — mintLocked always calls tsctx.cell ignoring entry_kind (thread_safe_reactive_map.zig:98), so its computed entries are cells; (3) moveTo desyncs on allocator failure — orderedRemove then insert-catch-return-false leaves the key in materialized but absent from order (reactive_map.zig:288-303).
- [ ] [#lazilyasynccomputedmapde] lazily-cs AsyncComputedMap defects: Len() is documented as reading through the membership plane but returns _order.Count, and _membership is written at :117 and never read anywhere (dead code, so Len is non-reactive); Remove (:92-98) never disposes the removed AsyncComputed, unlike both sync clear delegates (ReactiveMap.cs:299,:405), leaving readers on a stale cache. Also: no thread-safe or async SourceMap exists at all — conformance papers over it with a hand-locked sync map (MaterializationConformanceTests.cs:305) and a bare Dictionary (:336).
- [ ] [#lazilytwoimplementations] lazily-js: two implementations disagree on moveTo with a negative index — reactive-family.js:270 uses Math.min(index, Math.max(len-1,0)) with no lower clamp, collections.js:196 uses Math.max(0, Math.min(index, len-1)). moveTo(key,-1) splices before the last element in one and at 0 in the other. Untested in both; pick one before any unification silently changes a published surface.
- [ ] [#lazilydartcomputedmap] lazily-dart: ComputedMap entries are Slot, not Computed (collections.dart:430). Post-v2 Slot is the UNGUARDED pass-through while Computed carries the == memo guard — so ComputedMap does not have the guard its name implies, and this leaks publicly through handle(). Decide whether the handle becomes Computed.
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
