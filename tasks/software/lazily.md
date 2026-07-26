---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume: 24f78fda-2db5-41b6-8eb1-52772d913df8
agent: claude
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260726-174052.md`*

Compacted content:
- Archived 26 response topic(s): the done-archive quote bug is fixed, not worked around; Phase 0; Phase 1; 23 more
- Prior summary/context: prior compacted content: Archived 28 response topic(s): create lazily-cs (C#); complete all lazily-cs features; queue...

### Re: do [#lazilyupgradeconformance] — opus-5

Done in all three remaining bindings — rs, dart, zig — each committed, pushed, and gated green. The static grep is gone from the family except where a binding never had one.

**The upgrade earned its keep.** Every binding's mutation check ran the same probe: disable one fixture's replay while leaving its filename in the source. In all three the suite stayed green *and the old static guard stayed green* — rs at 118/124, dart at 111/124, zig at 94/124 — while the runtime guard failed and named the fixture. Same tree, opposite verdicts. That is the whole case for the change.

**What the manifests found, per binding:**

- **rs**: one real hole. `tests/crdt_tree_laws.rs` is `#![cfg(feature = "lossless-tree")]`, no `make check` target enabled that feature, and `test-lossless-tree` enumerates its binaries explicitly — so the entire file compiled to nothing while `crdt-tree/algebra.json` sat in the source where the grep counted it. Fixed by adding `--test crdt_tree_laws`; 118 named → 117 opened. The four `topiccell_*.json` I expected to fall out did not: `topic_conformance.rs` still hand-transcribes them, but the new `queue_family_conformance.rs` topic replays open all four, so the manifest sees them.
- **zig**: 22 fixtures fell out, 8 of them the real thing — inline mirror tests asserting against values typed into the `.zig` source with the fixture name only in a comment. The other 14 are vendored `@embedFile` copies, which a compile-time embed can never record; rather than excuse them, the agent added a test that opens the *canonical* file and asserts the vendored copy is byte-identical, so vendored drift now fails instead of silently replaying yesterday's corpus.
- **dart**: 111/124 unchanged — no named-but-never-opened fixture at all.

**dart's real finding was in the recorder, not the corpus.** `FileMode.append` in Dart is not POSIX `O_APPEND`: the end-of-file offset is captured when the handle opens, so concurrent isolates clobber each other. Its first manifest held severed id fragments (`sure.json`, `_and_leaf_edit.json`) that made the guard report three *intact* fixtures as never opened — the recorder manufacturing the exact drift it exists to detect. Two more attempts (fcntl locks are per-process, useless across `dart test` isolates) landed on an `O_EXCL` lockfile plus explicit seek-to-length: measured over 4 processes × 4 isolates × 3200 lines, plain append kept 2009 with 199 mangled; the final shape kept 3200 with 0. It also added a check the Go reference lacks — reject any recorded id naming no file in the corpus — which is now queued for the other three.

Two build-system traps worth recording: `zig build test` **caches Run steps**, so a second run skipped every test binary and left the freshly-truncated manifest empty (fixed with `has_side_effects` plus an explicit child env when the variable is set); and rs needed `conformance-manifest-reset` as the *first* prerequisite of `check`, which makes `make check -j` unsafe by construction.

Also closing `#lazilythreadsafetopiccel` and `#lazilythreadsafeworkqueu` — both shipped earlier today in lazily-rs 0.51.0 with all eleven canonical fixtures replayed per flavor and ten mutation probes confirmed.

**One finding I am not closing.** The rs agent hit `concurrent_push_and_read_do_not_deadlock` failing with `left: 199, right: 200` mid-run. It is not a deadlock — it is a lost or stale reactive read in `ThreadSafeQueueCell`, and it is pre-existing: 0/25 in isolation, but 2/30 on the working tree and 1/30 on stashed HEAD under saturating CPU load. That is a correctness defect in code released as 0.51.0, on the exact surface the thread-safe flavor exists to provide, so it goes to the top of the backlog rather than into this item.
<!-- agent:boundary:d0062d27 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- do [#lazilyzigconformance]
- do [#lazilyzigport]
- ~~do [#lazilyupgradeconformance]~~
- do [#lazilyconformancecoverag]
- ~~do [#lazilythreadsafetopiccel]~~
- ~~do [#lazilythreadsafeworkqueu]~~
- do [#lazilybenchmarkregressio]
- do [#lzqfjs]
- do [#lzqfports]
- do [#lzqfzig]
- do [#maphandlecleardependents]
- do [#lazilycellfamily]
- do [#lazilyspecdisagrees]
- do [#lzjsnpmtrustedpub]
- do [#lzcsqueues]
- do [#lzcscrdt]
- do [#lzcsipc]
- do [#lzcssync]
- do [#lzcstemporal]
- do [#lzcscluster]
- do [#lzcsplane]
- do [#lzcspackage]
- do [#asyncplanenever]
- do [#lazilyrenameinverted]
- do [#readmeexamplesreference]
- do [#signalgetsignallive]
- do [#recommendeddecidewhether]
- do [#lazilycppconformance]
- ~~do [#lazilycppsignalingroom]~~
- do [#lazilycppcollections]
- do [#trackedcopieslazily]
- do [#firstunificationwire]
- do [#prototypeunifiedreactive]
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
- [ ] [#lazilyzigconformance] lazily-zig: 8 conformance fixtures are asserted by INLINE MIRROR tests — values typed into the .zig source with the fixture name only in a comment, so upstream can change the fixture and the mirror stays green. Same defect class as lazily-cpp's hand-transcribed queue tests. The runtime manifest now names each one in KNOWN_UNCOVERED with its mirroring file: keyed_reconciliation_lis (reconcile.zig), mergecell_algebra (merge.zig), semtree_incremental (sem_tree.zig), stableid_alignment (stable_id.zig), anti_entropy_converge (crdt_plane.zig), crdt_sync_frames (crdt_plane.zig + ipc.zig), anti_spoof_session and frames (signaling.zig). Replace each mirror with a real fixture replay and shrink the ledger
- [ ] [#lazilyzigport] lazily go/rs/zig: port lazily-dart's corrupt-manifest-id check into the other runtime conformance guards. dart found that a recorder can mangle its own evidence (severed ids like sure.json) and make the guard report INTACT fixtures as never opened — a guard that cries wolf gets switched off. Its guard now rejects any recorded id that names no file in the corpus. go, rs and zig all lack that check. Cheap: one loop over the manifest before the coverage comparison
- [ ] [#lazilyconformancecoverag] lazily-cs: 87 of 124 canonical fixtures unreplayed (cs opens 37). MEASURED via the runtime manifest 2026-07-26. CORRECTION — an earlier note on this item claimed the 7 statechart fixtures were a cheap "runner gap"; that was derived from the STATIC grep and is WRONG. cs enumerates its corpus, so no fixture filename appears in its sources, and the static count under-reported every area. Runtime truth: cs FULLY replays reactive-graph 21/21, statechart 7/7, materialization 3/3, familysync 1/1. There is NO runner-gap win anywhere in cs — every one of the 87 is a genuinely missing surface, each already declared in a per-runner Unsupported ledger with the blocking surface named. Unreplayed by area: collections 15, lossless-tree 9, root 8, message-passing 8, reliable-sync 8, rateshape 6, coordination 5, resilience/service/temporal/windowing 4 each, presence 3, agent-doc/distributed/signaling 2 each, crdt-tree/membership/receipts 1 each. Work it down against tasks/software/lazily-cs-parity-plan.md by SHIPPING SURFACES, not by wiring runners
- [ ] [#lazilybenchmarkregressio] lazily-rs: the benchmark regression budgets have NO CI enforcement. ci.yml enumerates individual cargo test invocations and never runs `make check` or `benchmark-check`; regressions.yml runs only the loom model. So every budget in BENCHMARKS.md is enforced solely by whoever remembers to run `make benchmark-check` locally with a populated target/criterion — which, until #lazilyfailstarget, was nobody, because the gate hard-failed on a pruned target dir. Add a scheduled job (regressions.yml already has a nightly cron and is the natural home) that runs `cargo bench` then `make benchmark-check-strict`, so a real regression fails somewhere other than one developer machine. Note the numbers are machine-specific, so the job needs a stable runner class or relative-delta budgets rather than absolute ones
- [ ] [#lazilyasyncmap] lazily-rs async map: `observe` is still hard-bound to &AsyncContext (the UNTRACKED surface), so reading a keyed async entry from inside a compute cannot subscribe to it. CONFIRMED as a family-wide class, not an rs quirk: the same defect was found and fixed in go (its SINGLE-THREADED map's Keys/Len/ContainsKey called the zero-argument Source.Get(), documented in core.go as an untracked external read — probed directly, a reader still reported 2 after a third key was added), in cs (async Len read a membership cell that was written on every mutation and never read anywhere), and in kt (async Core reads spelled against the bare AsyncContext register no edge; this graph tracks through AsyncComputeContext). All three now take an explicit read surface. rs is the ONLY binding left where a keyed async entry read cannot subscribe. Fix shape is settled and shipped 8 times over: make the read generic over the read surface so a compute view registers the edge and a bare context does not
- [ ] [#maphandlecleardependents] MapHandle::clear_dependents exists only on the SYNC handle trait (lazily-rs cell_family.rs:134); the ThreadSafe and Async handle traits have no disposal hook. PARTIALLY RESOLVED 2026-07-26 — the blocker claim was wrong and is now disproven in practice. lazily-cpp shipped it: both thread-safe traits dispose through ctx.context() (BasicThreadSafeContext and AsyncContext each project onto a real Context, so no new primitive was needed), and AsyncSlotHandle gained a clear_dependents that drops the cached value since it has no graph dependents to clear. lazily-cs shipped it too, and found the reason it matters: AsyncComputedMap.Remove never disposed the removed node, so a reader holding the handle kept being served the removed entry's last resolved value. REMAINING: rs's own ThreadSafe/Async handle traits still have no hook; py has none on ANY flavor, so its removes drop the orphaned node without teardown consistently across all three. Note cs also needed a TryGetHandle before the disposal was observable enough to test at all
- [ ] [#lazilycellfamily] lazily-rs cell_family.rs:344-345 documents the present set as "only grows (deferral, not de-allocation)" ten lines above a `remove` that shrinks it (:325-334). Pre-existing doc bug, independent of any refactor — fix the comment or scope it to the materialization plane.
- [ ] [#lazilyspecdisagrees] lazily-spec disagrees with itself on the Dart thread-safe row: coverage.json marks the ThreadSafeComputedMap row shipped for Dart, while docs/coverage.md:85-87 declares the thread-safe context "not applicable" for Dart (single isolate, no shared mutable heap). Resolve in the spec — this is the same question as whether a thread-safe flavor is meaningful on a single-threaded runtime.
- [ ] [#lzqfzig] lazily-zig: the queue family is NOT graph-backed — QueueCell holds a *Context but mints no node; reader-kinds are five u64 version counters callers must poll. It scores green on the coverage row anyway. Make it graph-backed before adding flavors, or the flavor work lands green against nothing. Same finding as zig's sync ReactiveMap; consider one pass for both — plan: tasks/plan-lazily-queue-family-flavors.md
- [ ] [#lzqfjs] lazily-js: QueueCell/TopicCell/WorkQueueCell are not reactive at all — src/queue.js takes no context, contains ZERO ctx references, and reports invalidation by diffing a private snapshot. It scores green on the coverage row. Bind the family to a context and mint real reader-kinds, and check whether the collections fixtures are being satisfied by a non-reactive model here the way test/collections.test.js drives src/collections.js instead of the reactive families — plan: tasks/plan-lazily-queue-family-flavors.md
- [ ] [#lzqfports] lazily: Phase 5 — port the queue-family flavors to cpp, dart, py, go, kt (and build cs flavor-complete from the start under #lzcsqueues). One subagent per binding, NEVER two against one repo. Dispatch only after #lzqfspec + #lzqffx + #lzqfrs are committed so each agent has a settled spec, a red per-flavor gate, and a reference diff. cpp additionally needs its Source<uint64_t> version cells migrated if Phase 0 picks memoized-derive. KEPT OPEN 2026-07-26: #lzqfspec and #lzqffx are now committed, but #lzqfrs (the rs reference implementation) is not, so the stated precondition is unmet — dispatching now would be six independent guesses at an unpinned shape. Unblocks the moment #lzqfrs lands — plan: tasks/plan-lazily-queue-family-flavors.md
- [ ] [#firstunificationwire] FIRST, before any unification: wire the collections atomic-move fixtures (cellmap_atomic_move.json, cellmap_independence.json) into every binding replay harness against ALL THREE flavors. Today zig never replays them at all (only doc-comment references at reactive_map.zig:32,:39) and js replays them only against its fourth, non-reactive collections.js model — so the reactive moveTo has no fixture backing anywhere. Without this the unification lands green against nothing. Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#prototypeunifiedreactive] Prototype the unified ReactiveMap shape in lazily-zig and lazily-cpp — NOT lazily-cs. cs ships only 13 of 48 coverage rows (35 absent) and has NO thread-safe or async SourceMap at all; its conformance substitutes a hand-locked sync map (MaterializationConformanceTests.cs:305) and a bare Dictionary (:336). Prototyping a unification against two families that do not exist would invent the surface rather than converge one, and cannot surface what breaks. zig and cpp are feature-complete, can unify all three families, and neither exposes maps on its C ABI. Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#lazilyphaseextract] lazily-rs Phase A: extract the closure-free KeyedOrder<K,H> core (entries + order + membership + present-set + position/keys/len/contains_key + move_to/move_before/move_after) shared by all three families; thin per-model shells keep materialize/observe. Do NOT route through SyncReactiveGraph — its Send+Sync bound is stricter than the concrete Context API and would break Rc-capturing callers. `remove` stays out of the core (monotone present set is fixture- and Lean-backed). Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#unifythreemap] Unify all three map families in lazily-zig and lazily-cpp — both can fold async in, unlike the rest. zig: fn ReactiveMap(comptime Ctx,K,V,kind) with Ctx.Handle/Ctx.Observed/Ctx.Lock projections; the proof already exists in-tree as Engine(comptime Model) at reactive_graph_conformance.zig:1206 over SyncModel/TsModel/AsyncModel. cpp: traits class + existing duck-typed Cx param; only Source::clear_dependents(Context&) and Computed::clear(Context&) block it, and AsyncSlotHandle needs one added clear_dependents. Neither exposes maps on its C ABI. Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#lazilymapunification] lazily-go map unification BEFORE it hits v1 — still v0.22.0 on an unversioned module path, so a breaking change costs nothing now; after v1 it costs a /v2 module path plus every import rewrite. Mechanism: closure fields (already proven in-repo at reactive_map.go:92-96) or one extra type param. Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#zigdefectsfound] zig defects found while surveying, independent of unification: (1) the sync ReactiveMap is NOT graph-backed — stores plain V and fakes reactivity with u64 counters callers must poll, so keys()/len()/contains() register no edge (reactive_map.zig:140-419 touches ctx only for .allocator); (2) ThreadSafeComputedMap mints the WRONG node kind — mintLocked always calls tsctx.cell ignoring entry_kind (thread_safe_reactive_map.zig:98), so its computed entries are cells; (3) moveTo desyncs on allocator failure — orderedRemove then insert-catch-return-false leaves the key in materialized but absent from order (reactive_map.zig:288-303).
- [ ] [#lazilyasynccomputedmapde] lazily-cs AsyncComputedMap defects: Len() is documented as reading through the membership plane but returns _order.Count, and _membership is written at :117 and never read anywhere (dead code, so Len is non-reactive); Remove (:92-98) never disposes the removed AsyncComputed, unlike both sync clear delegates (ReactiveMap.cs:299,:405), leaving readers on a stale cache. Also: no thread-safe or async SourceMap exists at all — conformance papers over it with a hand-locked sync map (MaterializationConformanceTests.cs:305) and a bare Dictionary (:336).
- [ ] [#lazilytwoimplementations] lazily-js: two implementations disagree on moveTo with a negative index — reactive-family.js:270 uses Math.min(index, Math.max(len-1,0)) with no lower clamp, collections.js:196 uses Math.max(0, Math.min(index, len-1)). moveTo(key,-1) splices before the last element in one and at 0 in the other. Untested in both; pick one before any unification silently changes a published surface.
- [ ] [#lazilydartcomputedmap] lazily-dart: ComputedMap entries are Slot, not Computed (collections.dart:430). Post-v2 Slot is the UNGUARDED pass-through while Computed carries the == memo guard — so ComputedMap does not have the guard its name implies, and this leaks publicly through handle(). Decide whether the handle becomes Computed.
- [ ] [#recommendedspecdecision] [recommended] Spec decision for lazily-spec, blocking the family unification: (1) does the atomic-move contract in cell-model.md:275 bind EVERY flavor or only the source map? All nine bindings fail it on thread-safe and async, yet coverage.json marks all three rows green for every binding. (2) Is a thread-safe flavor meaningful on single-threaded runtimes? dart ships a 182-line duplicate family the spec marks N/A, and js README:633-640 argues the opposite of its own source comment at thread-safe.js:134-137.
- [ ] [#lazilycppconformance] lazily-cpp: 10 conformance runners are MARKER-ONLY — they open the canonical fixture but only assert its `"model"` string, then replay hand-transcribed steps (rateshape 6, coordination 5, temporal/windowing/service/resilience 4 each, presence 3, membership/crdt-tree 1 each = 33 fixtures). They pass a did-you-load-fixtures check while being blind to any change in the corpus steps or expectations. Convert to real replays; the coverage guard counts them as covered today.
- [ ] [#lazilycppcollections] lazily-cpp: 15 collections fixtures still uncovered — 9 with no coverage at all (cellmap_atomic_move, cellmap_independence, keyed_reconciliation_lis, semtree_incremental, stableid_alignment, textcrdt_convergence, textcrdt_delta_sync, workqueue_competing_delivery, workqueue_lease_deadletter) and 6 hand-transcribed (mergecell_algebra, 5x queuecell_*). Largest single body of remaining conformance work; spans five separate primitives. Tracked in the KNOWN_UNCOVERED ledger.
- [ ] [#trackedcopieslazily] The tracked copies of lazily-spec conformance fixtures inside lazily-py (tests/conformance), lazily-dart (test/conformance), lazily-rs (tests/conformance) and lazily-go (test/conformance) are kept byte-identical to canonical BY HAND with no sync script or drift check. This cycle had to flip the same fixture in three places. Add a sync/verify script (or make the runners read the sibling checkout) so a canonical edit cannot silently miss a vendored copy.
- [ ] [#asyncplanenever] Async plane never migrated to the v2 kernel: AsyncCellHandle / AsyncSlotHandle / AsyncSlotState survive in cpp, go, js, kt, py, zig, cs. Sharpest case is lazily-cpp `AsyncContext::memo()` (async_context.hpp:144), still live and undeprecated while the sync `Context::memo` is DELETED. Migrate to Async{Source,Computed} with deprecated aliases.
- [ ] [#lazilyrenameinverted] [recommended] lazily-py keeps `Cell`/`CellSlot` as the native class names with `Source = Cell` / `SourceSlot = CellSlot` aliases (cell.py:35,191,219-220). This is NOT drift — it is documented and motivated: cell.py is inside the mypyc compilation unit (setup.py compiles slot/cell/signal/effect/batch as one unit for cross-file native-class inheritance BaseSlot -> CellSlot), and the docstring states Cell "stays the native class name (for mypyc native-struct reads and isinstance stability across the family)". Flipping would rename the native class and change isinstance identity. Do NOT flip without deciding whether mypyc native-struct naming or family-wide vocabulary consistency wins; that is a spec/ownership question, not a cleanup.
- [ ] [#readmeexamplesreference] README examples reference types that no longer exist: lazily-go README:82 `lazily.NewCell` (never defined, example does not compile) and README:209 `*Cell[V]`; lazily-cpp README:54,280,387 + AGENTS.md:43 `CellHandle`/`SlotHandle` (deleted); lazily-dart README:38,329 claims a `Signal` back-compat alias that does not exist; lazily-py README:225 calls `CellSlot` canonical.
- [ ] [#signalgetsignallive] signal/getSignal still live and UNDEPRECATED on the threadsafe/async planes after the sync plane retired them: lazily-kt ThreadSafeContext.kt:195,247 + AsyncContext.kt:624, lazily-js reactive-async.d.ts:85, lazily-zig root.zig:86,90. Deprecate or migrate to eager Computed.
- [ ] [#recommendeddecidewhether] [recommended] Decide whether the domain *Cell family (QueueCell / TopicCell / RelayCell / MergeCell / TimerCell / LeaseCell / CircuitBreakerCell / PresenceCell / ...) follows the kernel to *Source, or is deliberately pinned as spec-level naming. Three independent bindings flagged it as the largest remaining Cell-vocabulary surface. It must move in lockstep across all 9 bindings, so it needs a decision before anyone starts. RECOMMENDATION 2026-07-26 (agent): KEEP *Cell. The kernel renamed the two value-node KINDS (Source/Computed) and kept Cell as the concept for a value-bearing reactive node. A QueueCell is not a kind — it composes them (five Source<u64> reader-version cells plus derived readers), so QueueSource would imply "written from outside, folds under a merge policy", which is false for a queue. Same for TimerCell/LeaseCell/CircuitBreakerCell/PresenceCell: each is a composite of a pure core plus projecting cells. KEPT OPEN: naming must converge with the operator before editing (9 bindings in lockstep + deprecation surface on every public type). Unblocks on a yes/no. If YES, scope is: spec cell-model.md first, then 9 bindings in lockstep with deprecated aliases, plus a decision on whether MergeCell collapses entirely (under the kernel it is already Source<T,M> with a non-default policy)
- [ ] [#lzjsnpmtrustedpub] lazily-js: register @lazily-hub/lazily-js as a trusted publisher on npmjs.com so release.yml can publish unattended. The workflow is correct — its OIDC exchange succeeds and provenance is signed and logged to Sigstore — but the upload is rejected with E404 PUT https://registry.npmjs.org/@lazily-hub%2flazily-js, meaning npm does not associate the package with that publisher (or the token lacks scope permission). This has now E404'd on two consecutive releases and each time required a manual 'npm publish --access public --otp=<code>' from the operator. Fix is npmjs.com-side configuration, not a code change
- [ ] [#lzcsqueues] lazily-cs: QueueCell + QueueStorage, TopicCell, WorkQueueCell, RelayCell with BackpressurePolicy/SpillStore/Transport — plan: tasks/software/lazily-cs-parity-plan.md (rows 12-14, 16)
- [ ] [#lzcscrdt] lazily-cs: TextCrdt + delta sync, SeqCrdt, CrdtTree, LosslessTreeCrdt incl. dotted-frontier anti-entropy and concurrent-merge convergence, LWW/MV registers, PnCounter, CellCrdt; replay crdt-tree + lossless-tree corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 17-24)
- [ ] [#lzcsipc] lazily-cs: Snapshot/Delta/CrdtSync wire types validated against the lazily-spec JSON Schemas, ShmBlobArena, BlobBackend zero-copy transport — plan: tasks/software/lazily-cs-parity-plan.md (rows 25-27)
- [ ] [#lzcssync] lazily-cs: CrdtPlaneRuntime + anti-entropy, resync coordinator, durable OutboxStore, IpcSink/IpcSource + full-duplex SyncDriver; replay reliable-sync + distributed + message-passing corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 28-32)
- [ ] [#lzcstemporal] lazily-cs: TimerCell/IntervalCell/CronCell/DeadlineCell over a logical clock, debounce/throttle/sample/probabilistic-sample, tumbling/sliding/session windows; replay temporal + rateshape + windowing corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 40, 41, 45)
- [ ] [#lzcscluster] lazily-cs: MembershipCell (SWIM + Phi-accrual), LeaseCell/LeaderCell/LockCell/SemaphoreCell/BarrierCell, presence/awareness/ephemeral, circuit breaker/retry/bulkhead/timeout, health/readiness/discovery/registry; replay membership + coordination + presence + resilience + service corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 42-44, 46-47)
- [ ] [#lzcsplane] lazily-cs: state projection/mirror, CausalReceipts, command-plane-v1, C-ABI FFI boundary, PeerPermissions/RemoteOp, SessionHandshake, instrumentation + benchmarks; replay receipts + signaling + agent-doc corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 33-39)
- [ ] [#lzcspackage] lazily-cs: multi-target netstandard2.1 / net8.0 alongside net10.0 (INumber<T> in the merge algebra needs a shim or policy split), add a release workflow, publish Lazily to NuGet — needs a NuGet API key from the operator
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily.done.md -->

<!-- completed work archived in tasks/software/lazily.done.md -->
<!-- /agent:done -->
