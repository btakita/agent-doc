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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260726-090849.md`*

Compacted content:
- Archived 28 response topic(s): create lazily-cs (C#); complete all lazily-cs features; queue + backlog attributes; 25 more
- Prior summary/context: /goal Please create lazily-cs (C#) implementation. Create gh lazily-hub/lazily-cs. Add a submodule to src/lazily-cs. ...

### Re: the done-archive quote bug is fixed, not worked around — opus-5

> **Queue prompt:**
>
> do [#lzdonequoteattr]


`#lzdonequoteattr` is done: `src/agent-doc` `32583dbcb`, pushed, `make check`
8152/8152, installed, controller marked to recycle at the next idle boundary.

**The defect.** `parse_attrs` kept attribute values verbatim, quotes included, on
the stated assumption that "values are unquoted (no quote support needed for
simple mode values)". That assumption was false about agent-doc's own writers: a
compact wrote `archive="tasks/software/lazily.done.md"` into this very document,
and from then on every preflight and session-check hard-failed with `must point to
a .done.md file` — because the parsed value ended in a quote character, so the
`.done.md` suffix check could never match. Earlier this session I only unquoted
the marker by hand to unblock the cycle; that was a workaround, and the parser
would have broken the next document a compact touched.

Values may now be quoted with `"` or `'`, and quoting also does what quoting is
for: `preset="two words"` no longer truncates to `two` under whitespace splitting.
An unterminated quote takes the rest of the input rather than failing closed —
one stray quote unparsing an entire marker is the same class of outage.

The fix lives in `parse_attrs`, so both done-archive validators pick it up from
one place; they already read through `attrs`.

**One finding worth recording: a test was pinning the defect.**
`parse_patch_with_transfer_source_attr` asserted the value *with* its quotes. The
buggy behaviour was the expected behaviour, which is a large part of why this
survived — anyone touching the parser would have seen a green test telling them
quote retention was intended. `transfer-source` turns out to have no production
consumer that wanted the quotes; the one attribute that did care was breaking
documents. I corrected the assertion and said why in the diff rather than quietly
flipping it.

**Coverage, mutation-checked.** Four `parse_attrs` unit tests (both quote
characters, quoted-value-with-spaces, quotes not disturbing following attributes —
using the real `preset="#spec-test-commit-push" priority go` marker from the
document that broke, and an unterminated quote) plus three against the actual
resolver: a quoted archive resolves, quoted and unquoted agree, and a
non-`.done.md` path is **still** rejected so the fix did not widen the check.
Reverting the strip turns four of five and two of three red respectively, and the
"still rejected" test correctly stays green — it should not depend on quoting.

**Two notes on the shared tree.** The first full-suite run failed on
`finalize_integration` with `[double-spawn] failed to exec … No such file or
directory` — the test binary vanished mid-run, which is the concurrent-build
symptom, not a regression; it passes in isolation and the re-run was clean. And
`src/agent-doc` still carries another session's in-flight work
(`.claude/skills/agent-doc/SKILL.md`, `.claude/settings.json`, `.codex/`,
`.opencode/`). I staged only my three source files and left all of it alone.

**Also confirmed since last cycle:** you published js, so
`@lazily-hub/lazily-js@0.29.0` is live with `latest` pointing at it. That closes
the release — all nine bindings published. The CI OIDC job for that repo will keep
404ing on `PUT` until its npm trusted-publisher config is finished, so treat a red
`Release` run there as expected rather than a regression and check npm directly.

Next head is `#lzqfspec` — Phase 0 of the queue-family plan.
<!-- agent:boundary:d33ad72c -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- ~~do [#lzqfprep]~~
- ~~⏭️ do [#lzdonequoteattr]~~
- ⏭️ do [#lzqfspec]
- ⏭️ do [#lzqfformal]
- 🚧 do [#lzqffx]
- do [#lzqfrs]
- do [#lzqfjs]
- do [#lzqfports]
- ⏭️ do [#lzqfzig]
- ⏭️ do [#maphandlecleardependents]
- ⏭️ do [#lazilycellfamily]
- ⏭️ do [#lazilyspecdisagrees]
- ⏭️ do [#lzjsnpmtrustedpub]
- ⏭️ do [#lzcsqueues]
- ⏭️ do [#lzcscrdt]
- ⏭️ do [#lzcsipc]
- ⏭️ do [#lzcssync]
- ⏭️ do [#lzcstemporal]
- ⏭️ do [#lzcscluster]
- ⏭️ do [#lzcsplane]
- ⏭️ do [#lzcspackage]
- ⏭️ do [#asyncplanenever]
- ⏭️ do [#lazilyrenameinverted]
- ⏭️ do [#readmeexamplesreference]
- do [#signalgetsignallive]
- do [#observerwordingcontradic]
- do [#renametreesource]
- do [#recommendeddecidewhether]
- do [#lazilycppconformance]
- do [#portconformancecoverage]
- do [#verifycrdtplaneruntimein]
- do [#lazilycppsignalingroom]
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
- ~~do [#portmembershiporder]~~
- do [#lazilyasyncmap]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lazilyasyncmap] lazily-rs async map: `observe` is still hard-bound to &AsyncContext (the UNTRACKED surface), so reading a keyed async entry from inside a compute cannot subscribe to it. CONFIRMED as a family-wide class, not an rs quirk: the same defect was found and fixed in go (its SINGLE-THREADED map's Keys/Len/ContainsKey called the zero-argument Source.Get(), documented in core.go as an untracked external read — probed directly, a reader still reported 2 after a third key was added), in cs (async Len read a membership cell that was written on every mutation and never read anywhere), and in kt (async Core reads spelled against the bare AsyncContext register no edge; this graph tracks through AsyncComputeContext). All three now take an explicit read surface. rs is the ONLY binding left where a keyed async entry read cannot subscribe. Fix shape is settled and shipped 8 times over: make the read generic over the read surface so a compute view registers the edge and a bare context does not
- [ ] [#maphandlecleardependents] MapHandle::clear_dependents exists only on the SYNC handle trait (lazily-rs cell_family.rs:134); the ThreadSafe and Async handle traits have no disposal hook. PARTIALLY RESOLVED 2026-07-26 — the blocker claim was wrong and is now disproven in practice. lazily-cpp shipped it: both thread-safe traits dispose through ctx.context() (BasicThreadSafeContext and AsyncContext each project onto a real Context, so no new primitive was needed), and AsyncSlotHandle gained a clear_dependents that drops the cached value since it has no graph dependents to clear. lazily-cs shipped it too, and found the reason it matters: AsyncComputedMap.Remove never disposed the removed node, so a reader holding the handle kept being served the removed entry's last resolved value. REMAINING: rs's own ThreadSafe/Async handle traits still have no hook; py has none on ANY flavor, so its removes drop the orphaned node without teardown consistently across all three. Note cs also needed a TryGetHandle before the disposal was observable enough to test at all
- [ ] [#lazilycellfamily] lazily-rs cell_family.rs:344-345 documents the present set as "only grows (deferral, not de-allocation)" ten lines above a `remove` that shrinks it (:325-334). Pre-existing doc bug, independent of any refactor — fix the comment or scope it to the materialization plane.
- [ ] [#lazilyspecdisagrees] lazily-spec disagrees with itself on the Dart thread-safe row: coverage.json marks the ThreadSafeComputedMap row shipped for Dart, while docs/coverage.md:85-87 declares the thread-safe context "not applicable" for Dart (single isolate, no shared mutable heap). Resolve in the spec — this is the same question as whether a thread-safe flavor is meaningful on a single-threaded runtime.
- [ ] [#lzqfspec] lazily-spec: Phase 0 of the queue-family flavor plan — add a Core-surface section for QueueCell/TopicCell/WorkQueueCell mirroring the ReactiveMap Core/Extended split, pin ONE reader-kind design (recommend memoized-derive+invalidate over cpp's version cells, and say explicitly that polling a counter is not a reader-kind), define the per-flavor obligation (confluence for thread-safe, eventual transparency for async, plus single-frontier-walk atomicity), resolve the threading paragraph that promises ThreadSafeContext no binding can accept, and split coverage.json's three conflated queue rows into nine — plan: tasks/plan-lazily-queue-family-flavors.md
- [ ] [#lzqfformal] lazily-formal: Phase 1 — prove the queue reader-kind invalidation set is a pure function of (op, len_before, len_after), prove one op's invalidations are not observably interleaved, and cross-check ThreadSafe.lean for the confluence obligation. Do NOT add per-flavor Lean models; the algebra must not fork — plan: tasks/plan-lazily-queue-family-flavors.md
- [ ] 🚧 [#lzqffx] lazily-spec + all binding runners: Phase 2 of the queue-family plan — replay the nine collections queue fixtures against all three flavors with a declared skip ledger (zig's reactive-graph runner is the reference: fails if everything skips, ledger must match observation). Add a positive assertion that fixtures ran, mutation-check each runner per flavor, and grep every queue runner for the invalidates-nesting trap (fixtures nest it under steps[].expected.invalidates; lazily-rs read it off the step and silently never asserted). Land this BEFORE any implementation so the gap goes red first. KEPT OPEN DELIBERATELY: this is Phase 2 and is gated on Phase 0 (#lzqfspec) settling the Core surface and the reader-kind design first — the plan's phases have a hard dependency order, and a queue drain that reaches this id before #lzqfspec is out of order, not ready work. Read the 'How these gates fail' section of the plan before writing any runner — plan: tasks/plan-lazily-queue-family-flavors.md
- [ ] [#lzqfrs] lazily-rs: Phase 3 — context prerequisites + reference implementation. ThreadSafeContext needs a multi-root clear_slots equivalent (it has only single-handle clear at thread_safe.rs:3549 — four handles is four frontier walks; verify whether batch() already closes this, do not assume). AsyncContext needs a synchronous-compute computed and a multi-root clear (it has neither; only computed_async at async_context.rs:413). Then ship ThreadSafe/Async QueueCell + TopicCell + WorkQueueCell, each minting its own reader-kind handles on its own graph, Arc<Mutex<S>> storage with invalidation run outside the lock — plan: tasks/plan-lazily-queue-family-flavors.md
- [ ] [#lzqfzig] lazily-zig: the queue family is NOT graph-backed — QueueCell holds a *Context but mints no node; reader-kinds are five u64 version counters callers must poll. It scores green on the coverage row anyway. Make it graph-backed before adding flavors, or the flavor work lands green against nothing. Same finding as zig's sync ReactiveMap; consider one pass for both — plan: tasks/plan-lazily-queue-family-flavors.md
- [ ] [#lzqfjs] lazily-js: QueueCell/TopicCell/WorkQueueCell are not reactive at all — src/queue.js takes no context, contains ZERO ctx references, and reports invalidation by diffing a private snapshot. It scores green on the coverage row. Bind the family to a context and mint real reader-kinds, and check whether the collections fixtures are being satisfied by a non-reactive model here the way test/collections.test.js drives src/collections.js instead of the reactive families — plan: tasks/plan-lazily-queue-family-flavors.md
- [ ] [#lzqfports] lazily: Phase 5 — port the queue-family flavors to cpp, dart, py, go, kt (and build cs flavor-complete from the start under #lzcsqueues). One subagent per binding, NEVER two against one repo. Dispatch only after #lzqfspec + #lzqffx + #lzqfrs are committed so each agent has a settled spec, a red per-flavor gate, and a reference diff. cpp additionally needs its Source<uint64_t> version cells migrated if Phase 0 picks memoized-derive — plan: tasks/plan-lazily-queue-family-flavors.md
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
- [ ] [#portconformancecoverage] Port a conformance-coverage guard to the 7 bindings that lack one. Only lazily-kt (scripts/check-conformance-coverage.sh) and now lazily-cpp fail the build when a canonical fixture is not actually replayed. rs, py, js, go, dart, zig, cs have no such guard, so the same marker-only / hand-transcribed drift found in cpp is undetected there. Audit each the same three ways: real replay vs marker-only vs absent.
- [ ] [#verifycrdtplaneruntimein] Verify CrdtPlaneRuntime ingest-op counting in the 7 bindings not yet checked. lazily-cpp returned the count of ops that CHANGED THE WINNER rather than ops accepted into the log (4 vs 5 on lww_last_writer_wins), breaking the spec rule that redelivery applies 0 new ops. lazily-kt (CrdtPlane.kt:148) is correct and cpp is now fixed; rs, py, js, go, dart, zig, cs are unaudited for the same divergence.
- [ ] [#lazilycppsignalingroom] lazily-cpp SignalingRoom::forward (include/lazily/signaling.hpp:203) silently drops frames addressed to an unknown peer, but conformance/signaling/anti_spoof_session.json requires an `unknown_target` error frame back to the sender. A faithful signaling runner stays red until this is fixed. The second signaling fixture (frames.json) additionally needs signaling wire serde, which lazily-cpp does not have.
- [ ] [#lazilycppcollections] lazily-cpp: 15 collections fixtures still uncovered — 9 with no coverage at all (cellmap_atomic_move, cellmap_independence, keyed_reconciliation_lis, semtree_incremental, stableid_alignment, textcrdt_convergence, textcrdt_delta_sync, workqueue_competing_delivery, workqueue_lease_deadletter) and 6 hand-transcribed (mergecell_algebra, 5x queuecell_*). Largest single body of remaining conformance work; spans five separate primitives. Tracked in the KNOWN_UNCOVERED ledger.
- [ ] [#trackedcopieslazily] The tracked copies of lazily-spec conformance fixtures inside lazily-py (tests/conformance), lazily-dart (test/conformance), lazily-rs (tests/conformance) and lazily-go (test/conformance) are kept byte-identical to canonical BY HAND with no sync script or drift check. This cycle had to flip the same fixture in three places. Add a sync/verify script (or make the runners read the sibling checkout) so a canonical edit cannot silently miss a vendored copy.
- [ ] [#asyncplanenever] Async plane never migrated to the v2 kernel: AsyncCellHandle / AsyncSlotHandle / AsyncSlotState survive in cpp, go, js, kt, py, zig, cs. Sharpest case is lazily-cpp `AsyncContext::memo()` (async_context.hpp:144), still live and undeprecated while the sync `Context::memo` is DELETED. Migrate to Async{Source,Computed} with deprecated aliases.
- [ ] [#lazilyrenameinverted] [recommended] lazily-py keeps `Cell`/`CellSlot` as the native class names with `Source = Cell` / `SourceSlot = CellSlot` aliases (cell.py:35,191,219-220). This is NOT drift — it is documented and motivated: cell.py is inside the mypyc compilation unit (setup.py compiles slot/cell/signal/effect/batch as one unit for cross-file native-class inheritance BaseSlot -> CellSlot), and the docstring states Cell "stays the native class name (for mypyc native-struct reads and isinstance stability across the family)". Flipping would rename the native class and change isinstance identity. Do NOT flip without deciding whether mypyc native-struct naming or family-wide vocabulary consistency wins; that is a spec/ownership question, not a cleanup.
- [ ] [#readmeexamplesreference] README examples reference types that no longer exist: lazily-go README:82 `lazily.NewCell` (never defined, example does not compile) and README:209 `*Cell[V]`; lazily-cpp README:54,280,387 + AGENTS.md:43 `CellHandle`/`SlotHandle` (deleted); lazily-dart README:38,329 claims a `Signal` back-compat alias that does not exist; lazily-py README:225 calls `CellSlot` canonical.
- [ ] [#signalgetsignallive] signal/getSignal still live and UNDEPRECATED on the threadsafe/async planes after the sync plane retired them: lazily-kt ThreadSafeContext.kt:195,247 + AsyncContext.kt:624, lazily-js reactive-async.d.ts:85, lazily-zig root.zig:86,90. Deprecate or migrate to eager Computed.
- [ ] [#observerwordingcontradic] "observer" wording contradicts the no-observers-only-Effects rule (#lzdartobservercow): lazily-cs Effect.cs:41,59 + Reactive.cs:119, lazily-dart core.dart:1386,1418 + state_chart.dart:13, lazily-zig effect.zig:12,202, lazily-js README:149, lazily-formal ThreadSafe.lean:9, lazily-kt ThreadSafeContext.kt:37. Effects are not observers; fix the prose.
- [ ] [#renametreesource] Rename the tree source files to match their types now that CellTree is SourceTree: lazily-rs src/cell_tree.rs, lazily-zig src/lazily/cell_tree.zig, lazily-cs src/Lazily/CellTree.cs + tests/Lazily.Tests/CellTreeTests.cs. lazily-cs precedent (81411c2) git mv'd its renamed files, so leaving them is a divergence from that repo's own convention.
- [ ] [#recommendeddecidewhether] [recommended] Decide whether the domain *Cell family (QueueCell / TopicCell / RelayCell / MergeCell / TimerCell / LeaseCell / CircuitBreakerCell / PresenceCell / ...) follows the kernel to *Source, or is deliberately pinned as spec-level naming. Three independent bindings flagged it as the largest remaining Cell-vocabulary surface. It must move in lockstep across all 9 bindings, so it needs a decision before anyone starts.
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
