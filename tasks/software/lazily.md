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

gated: npm security-key user presence required
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260730-015200.md`*

Compacted content:
- Archived 16 response topic(s): #lazilyrenameinverted; #lzinteropcs; #lzstdlibbarrier; 13 more
- Prior summary/context: prior compacted content: Archived 15 response topic(s): Response; do [#lzcstemporal]; do [#lzcscluster]; 12 more

### Re: do [#designimplementtransport] — opus-5

Done, all four phases, all three flavors, with spec + formal + conformance. `make check` green in `lazily-rs` and `lazily-spec`; `lake build` clean in `lazily-formal`.

**Answering your three questions directly:** yes to synchronous + threadsafe + async — that is the shape of the whole thing, not a follow-up. Yes to spec + formal + conformance — all three landed this cycle. And yes, an egress family is worth having; see the end of this note for why an Effect alone cannot be it.

#### What shipped

`lazily-rs f7d83c2`:

- **`src/ingress_core.rs`** — the graph-agnostic admission algebra. Same split `topic_core.rs` makes for the broadcast family and for the same reason: invalidation is a graph write, so the core performs none. Every mutator returns `IngressChange` — *which* reader kinds the transition dirtied — and each shell clears exactly that set on its own graph. Also carries the `IngressTransport` seam and `InProcIngress`: the core never touches a transport, so a WebSocket frame, an RPC response, and a polled page are the same input once decoded.
- **`src/ingress.rs` / `src/thread_safe_ingress.rs` / `src/async_ingress.rs`** — the three shells. Thread-safe runs invalidation outside the core lock and fans out through `batch()`; async uses `clear_slots` plus `AsyncContext::computed`, because **admission is not async-coloured** — the decision is a function of the fence, the watermark, the reorder buffer, and the observed clock, so there is nothing to await. Awaiting belongs to the transport, and the transport is outside the primitive by construction.

`lazily-spec aa689c2`: `docs/transport-ingress.md` plus seven fixtures in `conformance/ingress/`, and three `coverage.json` rows (Rust column only).

`lazily-formal 4f832bd`: `LazilyFormal/Ingress.lean`.

#### The design decisions that carry weight

**The admission order is the contract, not an implementation detail.** Lifecycle → generation fence → freshness → handoff → dedupe → ordering → backpressure → merge. Two of those orderings are load-bearing:

- The **fence outranks dedupe**. A zombie producer replaying old sequences under an old generation must be distinguishable from a legitimate retry; testing the sequence first reports `DuplicateSequence` and hides the zombie.
- **Freshness outranks ordering**. An expired envelope must never occupy a reorder slot, or a slow zombie starves live data out of a bounded buffer.

**Readiness, authority, and retry are derives.** Nothing polls a connection to ask whether it is healthy. Four reader kinds per scope, and the *negative* cases are the contract: a buffered out-of-order envelope invalidates nothing, a `tick` inside the freshness horizon invalidates nothing, an empty drain invalidates nothing. Meanwhile one admission that dirties several kinds clears them in a single frontier walk, so no reader ever observes "new value, old authority."

**Backpressure reuses the relay algebra** — `IngressPolicy.overflow` *is* `relay::Overflow`, and construction validates it against `M::CONFLATES` exactly as `RelayCell::new` does. `Block` refuses **without** advancing the watermark, which is what makes the producer's retry in-order rather than a duplicate.

#### The formal model earned its keep immediately

I set out to prove `buffered_is_invisible` unconditionally. It would not go through — and the reason was a real defect in the Rust I had just written: a generation handoff that arrives **out of order** resets the fence, the watermark, *and* the window, then parks the envelope. My code reported that as an ordinary invisible buffer, so every reader stayed pinned to the superseded generation's value forever.

Two fixes followed, and both make the code stricter than my original claim:

1. The buffered path now **derives** its invalidation set from a before/after state diff rather than asserting invisibility. "A buffered envelope invalidates nothing" is now a computed fact with two honest exceptions (the scope's first appearance, and a handoff that buffers).
2. A **refused** envelope no longer materializes a scope at all. An expired or blocked message for an untracked key is not an admission plane, and creating one reported a readiness change that never happened.

The theorem is now stated with its real precondition (`e.gen = s.gen`), and `handoff_that_buffers_resets_the_baseline` pins the other case. Both the unit suite and the corpus gained a regression for it.

The other result worth naming: **`reorder_needs_no_commutativity`**. `Relay.reorder_adjacent` needs `Commutative` to tolerate reordering, because a relay merges ops in *arrival* order. Ingress does not — the reorder buffer replays in *sequence* order, so a merely associative `⊕` converges to the in-order fold. The buffer buys what the algebra would otherwise have to pay for, and that is a genuine reduction in what a binding's merge policies must satisfy.

#### Conformance: what stops it reporting green while testing nothing

`tests/ingress_family_conformance.rs` replays all seven fixtures against all three flavors through one `IngressModel` trait. The flavor axis is in the runner, not the corpus.

- `invalidates` is asserted **per reader kind, in both directions**, through a cache-validity probe. A step expecting `false` fails if the shell invalidated anyway, so over-invalidation is as visible as under-.
- Every replay returns its step count and each flavor asserts it equals the corpus total. An absence guard proves fixtures exist; only a positive count proves this binary opened them. The runtime manifest confirms it: `129/134 canonical fixtures OPENED`.
- A three-row ledger is enforced by grepping `src/` in **both** directions, so a row cannot claim a flavor that does not exist and a flavor cannot exist unreplayed.
- `the_invalidation_probe_discriminates` pins the probe itself, since the corpus depends on it being able to fail.

**Seven mutation probes, all killed** (each applied, gate run, reverted with an mtime bump — a restore that preserves mtime lets cargo reuse the mutated build and reports a false green):

| Defect | What went red |
|---|---|
| fence checked after dedupe | `ingress_generation_handoff` — reports `duplicate_sequence` |
| handoff keeps the superseded window | `ingress_generation_handoff` — window value |
| `Buffered` marks every reader dirty | every `invalidates: false` step, all three flavors |
| `tick` marks readiness unconditionally | `ingress_freshness_and_retry` in-horizon tick |
| `Block` advances the watermark | `ingress_backpressure` retry reads as a duplicate |
| thread-safe `apply` clears outside `batch()` | frontier-walk gate — two effect runs for one admission |
| error-receipt channel never cleared | `invalidates.receipts.error` |

That last one is why `invalidates` is asserted per channel rather than by receipt **count**: a stale cache recomputes to the right count, so a count-only gate would have called it green.

#### On an egress family — and on "could it be an Effect?"

Yes, and the Effect question has a precise answer: **an Effect is the right attachment, but it cannot be the family.** An Effect has no state — no sequence assignment, no unacked window, no ack watermark, no retry budget, no ordering. Building egress *as* an Effect would put delivery state in a closure and lose exactly what ingress just made derivable.

The symmetric shape is `EgressCore` + three shells, where the Effect is only the sink attachment that reacts to `has_pending`:

- `EgressCore` owns sequence assignment, the unacked window, `acked_through`, and the retry budget — flavor-neutral, nothing awaited, returning its own invalidation set.
- The shells expose `pending` / `inflight` / `acked_through` / `retry` as `Computed`s, and **one Effect per attached transport**. That keeps the no-observers-only-Effects rule intact without pretending an Effect can hold delivery state.
- Most of the send side already exists and should be reused rather than reimplemented: `Outbox`/`RelayCell` for conflation and backpressure, `SpillStore`/`DurableOutbox` for durability and crash replay, `Transport` for framing. The genuinely missing piece is the mirror of what ingress just added — keyed lifecycle scopes, a generation fence on the **send** side (so a reconnected producer cannot replay under a stale incarnation), and ack-driven retry as a derive.

Queued as `#lzegress` with that shape written down, rather than started here — it is new scope beyond this item, and it deserves its own spec + formal + corpus the way ingress got.

#### What I deliberately left open

Phase 4 said "add cross-language conformance schedules … then project the contract into maintained Lazily bindings." The schedules are done — corpus authored, replayed, mutation-checked. The **projection into the other 8 bindings** is not, and it is 8 repos of real work rather than a remainder I should have folded in silently. It is queued as `#lzingressbindings` at the top of the backlog, actionable and unblocked, with the spec doc as its contract and the corpus as its gate. I closed the parent because everything the reference owes is landed and verified; say the word if you would rather I had left it gated.

CI on `lazily-rs` was `in_progress` on `f7d83c2` when I checked — recorded, not waited on.

### Re: do [#lzingressbindings] using subagents — opus-5

Done. All eight bindings, all three flavours each, the whole corpus replayed against every flavour. Nine bindings now ship the ingress family and the three `coverage.json` ingress rows are green across all nine columns.

Eight subagents, one per repo, one commit each:

| Binding | Commit | Flavours | Gate |
|---|---|---|---|
| py | `f5b4666` | `IngressCell` / `ThreadSafeIngressCell` / `AsyncIngressCell` | 650 tests, 130/134 fixtures |
| kt | `53a51c7` | all three | 413 tests, 131 fixtures |
| js | `a7db768` | all three | 568 tests, 131/134 |
| dart | `2bbedfa` | all three | 621 tests, 128/134 |
| zig | `678655e` + `636feba` | all three | 827 tests on 0.16.0, 438 on master |
| go | `463b1a2` | all three | full suite under `-race` |
| cpp | `7cf7115` | all three | 52/52 ctest, 112 fixtures |
| cs | `c4779de` | all three | 229 tests, **134/134 fixtures** |

`lazily-spec c121aca` flips the marks and records what the projection settled. Superproject gitlinks bumped in `c19de14` + `1facc7c`.

#### I did not take the reports at face value

For each binding I independently removed or weakened the generation fence, ran that binding's own gate, confirmed it went red, then reverted with an mtime bump and confirmed green again. All eight discriminate. In cpp I also checked the *exit code* rather than the printed `FAIL:` line, since a suite that prints failures and exits 0 is exactly the shape of a gate that reports green while testing nothing: it aborts with 134 and CTest marks it failed.

#### Three things the projection turned from implementation detail into contract

These are now written into `docs/transport-ingress.md`, because each one is a way a binding could pass while testing less than it claims.

**`invalidates` is a claim about observability, not about a mechanism.** Three probes satisfy it across the nine bindings — a cache-validity read, a recompute counter, and a version-source delta — because three graphs expose different things. All are bidirectional and all kill the same mutations. What is *not* acceptable is asserting receipt **counts** in place of per-channel invalidation: a stale cache recomputes to the right count, so a count-only gate reports green. Two bindings independently found the ordering matters too — with counts asserted first, the never-cleared-error-channel probe reports the count mismatch, which is the symptom rather than the defect. cs reordered its replay loop to assert invalidation first for that reason.

**"One frontier walk" has to be stated as "no torn observation", not as an effect-run count.** This is the most useful thing the fan-out produced. The Rust probe is "two effect runs for one admission"; js measured that its kernel *already* re-runs an effect once more when it lazily refreshes a second dirty dependency mid-run — 2 runs for one `clearComputeds([a,b])` walk with no ingress code involved. The gate as specified would have passed either way. What a second walk actually costs is a reader seeing `new value, old authority`: one walk yields `[[5,1],[9,2],[9,2]]`, two yield `[[5,1],[9,1],[9,2]]`, where `[9,1]` is a post-handoff value under the superseded fence. That is the invariant; the run count was one binding's proxy for it. Had that agent copied the Rust phrasing it would have shipped a probe that cannot fail.

**Admission is not async-coloured; reads may be.** Every mutator is synchronous in all nine bindings and no runner has a `settle` step — the finding held everywhere. But js, cs and zig have async graphs with no synchronous compute constructor, so their four scope derives project as `Promise` / `Task` / pending-queue reads. That is the graph's property, not the primitive's, and it does not weaken the row: the requirement is that *admission* is synchronous and that one admission is one walk.

#### Four bindings were already red before starting, for the same reason

py, go, js and cs each had a **failing** `make check` at HEAD: `conformance coverage FAILED: 7 problem(s)`, one per ingress fixture that existed in the corpus and was opened by nothing. That guard is a runtime manifest, not a grep, so it could not have gone green without the bytes actually being read. Last cycle authored the corpus with no consumer, and the guard correctly refused to call that covered — the strongest argument yet for the manifest over the static grep it replaced.

#### Two CI reds fixed, one left standing

**`lazily-rs` signaling worker** (`d815152`) — red on the last three runs, including two that predate the ingress work, and *not* the Rust job. `signaling/test/protocol.test.ts` imports the canonical signaling fixtures as ES modules from a sibling `lazily-spec` checkout; the Rust job clones that sibling, the signaling job never did, so `tsc --noEmit` failed TS2307 before a single test ran. It passed locally only because the sibling exists in this workspace — which is exactly why `make check` could not catch it. Now green in CI.

**`lazily-zig` 0.16.0** (`636feba`) — the one real defect the fan-out shipped. The corpus runner reflected its fixture keys through `@typeInfo(E).@"enum".field_names` / `.field_values`. Those exist on Zig master and **not** on 0.16.0, which exposes `.fields` instead — and master does not expose `.fields`. The agent verified on master only, so the suite compiled locally and failed to compile in CI. `std.enums.values` + `@tagName` is stable across every pinned toolchain, so the keys stay reflection-derived without welding the suite to one compiler. Verified both ways: 827/827 on 0.16.0, 438/438 on master.

**`lazily-zig` 0.15.2 is still red and I am leaving it.** `build.zig` reads `b.graph.environ_map`, absent in that version, and behind it `src/lazily/cell.zig` hits a dependency loop. Both predate the ingress family — `5d19ab7` had all three zig jobs red. I tried the one-line env-read fix and reverted it: `std.process.getEnvVarOwned` does not exist on 0.16/master either, so the shim would have broken two green jobs to not-fix a third. Filed as `#lzzig0152`.

CI at hand-off: spec, kt, js, dart, go, cpp and cs green; rs still running its Rust job (its signaling job already green); zig re-running on the fix. py has no workflow named CI. Recorded, not polled.

#### One thing I found that is worse than it looks

While flipping the coverage marks I checked the neighbouring rows. **Rows 21, 23 and 24 — async `TopicCell`, thread-safe and async `WorkQueueCell` — read `—` for every binding but Rust, yet py, kt and dart demonstrably ship those types**, and their own conformance ledgers claim all nine cells. This is ledger rot running backwards: the table understates what exists rather than overstating it, so nobody notices, and a "project this family into the bindings" task reads as unstarted work that is in fact partly done. Filed as `#lzcoverageaudit` rather than fixed inline, because clearing it honestly means proving per binding that each shipped flavour is actually *replayed* against the corpus — the same work ingress just did, not a mark flip.

### Re: do [#lzcoverageaudit] + do [#lzzig0152] — opus-5

Both done. The audit found the table wrong in **both directions at once**, and the Zig 0.15.2 job turned out to have four faults rather than the two the item knew about — including one that says something about the engine, not the toolchain.

I also closed out `#lzingressbindings`. Its response is committed at HEAD but its queue head and backlog entry were never struck — last cycle's closeout was interrupted mid-write, so the item read as unstarted work that was in fact landed. Exactly the failure mode the audit is about, one layer up.

#### The audit: 33 cells corrected, in both directions

`lazily-spec 326f25e`. Columns are rs / py / kt / js / dart / zig / go / cpp / cs.

| Row | Was | Now | What was wrong |
|---|---|---|---|
| 16, 19, 22 — single-threaded queue / topic / work-queue | js `~`, zig `~` | ✅ | partial with **no recorded reason** |
| 17, 18 — thread-safe + async `QueueCell` | js ✅ | `—` | **overstated — js ships neither type** |
| 17, 18 | go `—`, cpp `—` | ✅ | understated |
| 20 — thread-safe `TopicCell` | js ✅ | `—` | **overstated** |
| 20 | go `—`, cpp `—` | ✅ | understated |
| 21 — async `TopicCell` | py/kt/dart/go/cpp `—` | ✅ | understated |
| 23, 24 — thread-safe + async `WorkQueueCell` | py/kt/dart/go/cpp `—` | ✅ | understated |

**24 understated, 3 overstated, 6 unexplained partials.** The understated direction is the more dangerous one, and it is worth naming why: nobody files a bug about coverage they are told they lack. lazily-go has shipped `async_queue_family.go`, `thread_safe_queue_family.go` and a 43K `queue_family_conformance_test.go` that replays all three primitives against all three flavors — with an enforced 3×3 capability ledger — while six of its cells read absent. A "project this family into the bindings" task reads as unstarted work that is already done.

**The 3 overstated cells are the ones I would want to know about first.** lazily-js was recorded as shipping the thread-safe and async `QueueCell` and the thread-safe `TopicCell`. It ships none of them. Its own ledger records all three as `shipped: false` and greps `src/` in both directions to keep that honest — so the binding was actively asserting the opposite of what the canonical table claimed, and the table won by default because nothing compares them.

**On the 6 `~` marks: lazily-cs is the control case that settles them.** I could not recover what "partial" was supposed to encode — no note, no schema field for one. The tempting reading is "ships 1 of 3 flavors," but cs also ships only the single-threaded flavor and reads ✅ on exactly those three rows. So `~` on js and zig was drift, not a family-level annotation, and the fix is to make them read like cs.

#### How each cell was proved

Not by reading ledgers — by running each binding's suite this cycle:

| Binding | Evidence |
|---|---|
| py | `test_queue_family_conformance.py` 35 passed; ledger asserts 3×3 all shipped |
| kt | `QueueFamilyConformanceTest` green; `Flavor.entries` × `Primitive` exhaustive, zero staged skips |
| dart | 10 passed — named `(single-threaded)` / `(thread-safe)` / `(async)` per primitive |
| go | `TestQueueCell/TopicCell/WorkQueueCellCorpusAllFlavors` — 31 / 29 / 18 steps asserted per flavor |
| cpp | 4 ctest targets pass; ledger prints "9 shipped flavors, 11 fixtures read" |
| js, cs, zig | single-threaded replays green; absence proved by each ledger **plus** a `src/` grep for the type |

The absence direction matters as much as the presence direction: a row cannot claim a flavor whose type does not exist, and a type cannot exist unreplayed. Both directions are enforced inside the bindings.

#### lazily-zig needed the replay written before its rows could move

This is the case the item's rule was written for. zig ships `TopicCell` and `WorkQueueCell`, and had tests *named* after the canonical fixtures — `test "WorkQueueCell competing delivery fixture"` — whose expected values were **transcribed into the source**. The six fixtures sat in `KNOWN_UNCOVERED` with the reason "No runner at all in this binding." Accurate, and unnoticeable: a transcription cannot fail when the corpus changes upstream.

So `lazily-zig bbb3a24` adds `src/lazily/queue_family_conformance.zig` — all six fixtures replayed against the real cells, asserting full expected state, every return value, and the `invalidates` matrix **per reader kind in both directions**, so over-invalidation is as visible as under-. Each entry point returns its step count and the tests assert the totals (29 topic, 18 work-queue), because a runner that opens every fixture and drives none of their steps reports the same "ok" as a real one. The runtime manifest went 104 → **110/134 fixtures opened**; those bytes were really read.

Mutation-checked both primitives, and the first attempt taught me something: bumping the topic reader *after* `advance`'s no-op early return is unreachable for the steps that matter, so it passed. Moving the bump *before* the early return produces `topiccell_offline_tail_bounds step 3 (advance): subscriber worker invalidates want=false got=true`. On the other side, making `push` always dirty `is_empty` reddens `workqueue_competing_delivery` step 1 *and* the probe-discrimination test.

Two places the corpus is thinner than the code, recorded rather than papered over: `WorkQueueCell`'s visibility timeout and `max_deliveries` are not in the fixture `initial`, so I used lazily-go's values rather than tuning until zig passed; and `restart` carries a `subscriber` the observable contract never uses.

#### Zig 0.15.2: four faults, and the interesting one is not a toolchain quirk

`lazily-zig 4438486`. The item knew about `b.graph.environ_map` and the `cell.zig` dependency loop. Behind those:

1. **`b.graph.environ_map`** is a *rename* — 0.15.2 spells it `env_map` — so `buildEnvVar` asks the compiler which field the toolchain declares. Confirming the item's warning: `std.process.getEnvVarOwned` does not exist on 0.16/master, so that shim trades one red job for two.

2. **The graph mutex gate was backwards, and this is the real finding.** `context.zig` picked `std.Thread.Mutex` below 0.16 and the vendored `ReentrantMutex` above. But `Slot.initKeyed` calls user `valueFn` *while holding the graph lock*, and a `valueFn` re-enters via `cell()`/`slot()` — reentrancy is a **requirement of the engine**, introduced deliberately (releasing the lock before `valueFn` opened a use-after-free). The 0.15 arm kept a non-reentrant lock, so it deadlocked the moment a compute touched a second cell; 0.15.2's Mutex detects the re-acquire and panics "Deadlock detected", while the newer toolchains never executed that path at all. The version split wasn't protecting 0.15 — it was hiding that 0.15 had a different, broken concurrency contract. The vendored lock compiles there unchanged, so the gate is gone and all three versions now share one lock.

3. **`cell.zig` dependency loop** — a field typed `?DeinitFn` makes field resolution depend on a container decl naming `*Self`. Spelling the pointer inline is the same type with no decl edge. Fixing fault 4 first also made this error stop cascading, which is why it looked like two problems.

4. **`Compute.tryGet` declared `error{StaleCompute}!@TypeOf(handle.get())`.** When the handle's read is itself fallible — `TopicCell.Reader.get` is — that asks for a *nested* error union. 0.15.2 refuses the coercion; later toolchains happen to accept it. `TryGetResult` merges the error sets into one flat union with the same reachable errors.

Plus two genuinely version-absent std surfaces: `mmap`'s `prot` is a `u32` bitmask on 0.15.x and a packed struct after, and `std.Io` on 0.15.2 is only the Reader/Writer namespaces — so `FileOutboxStore` is *unbuildable* there rather than untested. Its two tests skip visibly; nothing else in `reliable_sync.zig` is downgraded.

**I walked into the exact trap the item warned about.** My first `mmap` fix reflected on the function signature to avoid naming either `prot` form — and `Type.Fn` spells its parameter list `params` on 0.16 and `param_types` on master, so the reflection needed the same version gate the argument did. master went red. The lesson the item recorded about verifying on one toolchain applies to the *fix* as much as to the code.

All three pinned toolchains, before and after the new conformance file: **0.15.2 768/772 (4 skipped), 0.16.0 830/830, master 441/441** — each up exactly 3 from the new tests. `make check` green including the coverage guard. Mutation-checked that 0.15.2 discriminates rather than passing by omission: dropping the closed-reader bump in `QueueCell.close` turns `queuecell_closure_lifecycle` red *there*.

#### CI, and one more thing the item implied

The zig CI run on `bbb3a24` is green on all four jobs — **including `Zig 0.15.2`**. But the job carried `continue-on-error`, so it could not have failed the build either way. A job that cannot fail is indistinguishable from a job that passes, which is the same shape as a suite reporting green while testing nothing; keeping the gap "visible" only works if someone reads a green checkmark as red. `1ec2a45` removes it, so 0.15.2 gates again. That is what "restore the job" has to mean.

Twelve repos committed and pushed, superproject gitlinks bumped in `8b0eb2831` + `d20c8e4`. The nine binding README bumps are generated by `sync-coverage.mjs`; CI's `--check` fails on drift, so they are not hand edits.

#### What this leaves, filed rather than folded in

The corrected table now shows **18 honestly-absent cells**: js, zig and cs ship no thread-safe or async queue-family flavor. That gap was previously masked — three cells by a false green and six by unexplained partials — and it is the same projection work `#lzingressbindings` just did for ingress. Queued as `#lzqueuefamilyflavors`. The work-queue fixture's missing lease configuration is `#lzwqfixtureconfig`; it is small, but every binding independently hardcoding the same two numbers is a corpus gap, not a binding quirk.

zig still lists 24 known-uncovered fixtures. Those are whole families with no runner (lossless-tree, message-passing, seqcrdt/textcrdt) rather than transcriptions masquerading as replays, so they are a different and larger piece of work than this item.
<!-- agent:boundary:a0d54382 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- 🚧 do [#lzqueuefamilyflavors]
- do [#lzwqfixtureconfig]
- do [#lzingressadapters]
- do [#lzgdbinding]
- do [#lzcppwasm]
- do [#lzcoveragelayout]
- Fix the red test suites in the lazily implementations.
- do [#lzegress]
- do [#kny1]
- Research https://github.com/Cysharp/R3 and if lazily can have an adapter. Benchmark lazily-cs vs R3.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] 🚧 [#lzqueuefamilyflavors] Ship the thread-safe and async queue-family flavors in lazily-js, lazily-zig and lazily-cs — the 18 cells that coverage.json rows 17/18/20/21/23/24 now honestly show absent, previously masked by a js false green and by unexplained partials on js/zig. Same shape as #lzingressbindings: a ThreadSafeQueueCell/AsyncQueueCell, ThreadSafeTopicCell/AsyncTopicCell and ThreadSafeWorkQueueCell/AsyncWorkQueueCell per binding, then extend that binding's existing queue-family ledger (which today records them shipped:false and greps src/ in both directions) and drive the canonical queuecell_/topiccell_/workqueue_ corpus against each new flavor with per-reader-kind invalidation asserted in BOTH directions. Mutation-check each new flavor before flipping a mark; a ledger going green on a flag flip alone is the exact false green those ledgers exist to prevent.
- [ ] [#lzwqfixtureconfig] Put the WorkQueueCell lease configuration into the canonical workqueue_*.json fixtures' initial block: visibility_timeout and max_deliveries are absent, so every binding hardcodes them out of band (10, and max_deliveries 3 for competing_delivery vs 2 for lease_deadletter) and a new binding has to read lazily-go's runner to discover them. Add the fields to the fixture and the schema, then update the nine bindings' runners to read them, so a fixture change cannot silently mean something different in each binding.
- [ ] [#lzegress] Design and implement the egress family as the mirror of ingress (#designimplementtransport): an EgressCore owning sequence assignment, the unacked window, the ack watermark, and retry budget, with sync + thread-safe + async shells exposing pending/inflight/acked_through/retry as Computeds and ONE Effect per attached transport as the sink attachment. An Effect alone cannot be the family — it has no delivery state — so the Effect attaches, the core decides. Reuse Outbox/RelayCell for conflation+backpressure and SpillStore/DurableOutbox for durability rather than reimplementing them; add a generation fence on the SEND side so a reconnected producer cannot replay under a stale incarnation. Spec + formal + conformance corpus alongside, as ingress did.
- [ ] [#lzingressadapters] Extend the shipped IngressCell family with boundary-ingress adapters learned from the agent-doc 0.35.100 build; do not create a second ingress primitive. Phase 1: specify an event-channel-first adapter contract for sync, thread-safe, and async sources, with RPC/poll adapters allowed only at external non-reactive boundaries and all ingress updating Sources. Phase 2: provide atomic snapshot-plus-event bootstrap with monotonic cursors, gap detection/cold replay, generation fencing and hot resubscription so controller or producer replacement cannot strand a graph on an old stream. Phase 3: derive readiness, authority, freshness, membership, delivery convergence and template-style validation in Computeds (extract every reactive read into the computed); keep subscription lifecycle, I/O, acknowledgements, retry and commits in Effects. Include keyed SourceMap membership and one-shot durable delivery receipts so temporarily reaching zero subscribers is not mistaken for convergence. Phase 4: map agent-doc onto the adapters as the reference integration, then add spec, formal model, cross-binding conformance fixtures and mutation probes for restart during bootstrap, stale-generation events, cursor gaps, zero-member delivery, duplicate acknowledgements, backpressure and no-torn-observation. Reuse #designimplementtransport semantics and coordinate with #lzegress rather than duplicating either core.
- [ ] [#kny1] Please help me setup trusted publishing with npm.
- [/] [#lzjspublish] After the operator completes npm security-key reauthentication in the Brave package-access tab, configure the GitHub Actions trusted publisher for lazily-hub/lazily-js using workflow release.yml with npm publish permission; rerun release job 30309980372 for v0.30.1 and verify the registry resolves 0.30.1.
- [ ] [#lzgdbinding] Implement lazily-gd, a pure-GDScript binding, as the 10th coverage.json column per tasks/software/plan-lazily-gd.md. Staged entry copying lazily-cs: rows 0 (reactive graph) + 15 (stable-id) first, KNOWN_UNCOVERED populated and shrinking per phase. The one novel problem is that RefCounted has no cycle collector — no other binding faces this — so back-edges are WeakRef, which makes ctx.scope() the SOLE strong owner of effects and requires prune-on-propagate before any edge-count assertion and an edge index keyed on get_instance_id(). Port the runtime-manifest guard (build/conformance-fixtures-loaded.txt + check-conformance-coverage.sh with EXTS=.gd) in the SAME phase as the runner, and mutation-check it before trusting green. Efficient loading is a Phase-1 exit criterion, not a late phase: a load-graph test asserting the kernel loads no other family, since preload() is eager+transitive and retrofitting layering after six families exist is the expensive order. Ship as addons/lazily/ with the runner and 134 fixtures export-excluded. BLOCKED on the operator choosing a minimum Godot 4.x version.
- [ ] [#lzcppwasm] Give lazily-cpp an Emscripten/wasm32 target with a replayed feature-coverage matrix and benchmarks per tasks/software/plan-lazily-cpp-wasm.md. Measured: the reactive core is already wasm-clean (cell.hpp closure = 8 headers, zero POSIX and zero threads; same for collections/keyed_order/statechart), work_queue.hpp needs -pthread, and the lazily.hpp umbrella (46 headers) drags in shm_open/mmap via transport.hpp — so three explicitly-labeled build tiers (core / threaded / native-only-excluded), not one build that silently drops features. Add lazily/core.hpp first to name the narrow-include contract. The coverage matrix must be generated from the runtime fixture manifest, never hand-authored: lazily-cpp's own queue tests were previously hand-transcribed rather than replayed, which is exactly the drift to avoid repeating on a new target. Wasm benchmarks go in a SEPARATE BENCHMARKS.md section with their own environment line — native -O3 numbers are not comparable. Do NOT split the large headers (context.hpp 2117 lines etc.): header size costs compile time not runtime loading, the dependency hazard is already solved at file granularity, and it would churn a released library for no behavioral gain. Do NOT flip coverage.json — it tracks language bindings, not build targets of one binding.
- [ ] [#lzcoveragelayout] Restructure the coverage.json feature-table layout for a growing language axis per tasks/software/plan-lazily-coverage-table-layout.md. Measured: the feature column is 68% of row width at mean and 84% at max (feature text mean 86 / max 203 chars vs ~40 for all nine marks combined), so a 10th language adds ~4 chars to a row already 130-250 wide — the language axis is NOT the problem. Reject transposing (64 columns) and reject splitting by language: sync-coverage.mjs injects the same full matrix into all 12 targets, and per-language tables would destroy the cross-binding parity view that check-conformance-coverage.sh exists to protect. Phase 1 (land BEFORE the GDScript column, independent of the rest): add id + short label per row and move the 86-203 char normative prose to footnotes or into cell-model.md/protocol.md, taking the table from ~250 to ~60 chars wide so the layout scales past 15 languages. Phase 2 with it: split the '— absent OR not applicable' mark, since GDScript's parked thread-safe family is a different fact from unwritten work. Phases 3-4 (follow-up): author a family field — 54 distinct prefixes across 64 rows means families CANNOT be inferred from the current text — then a family x language roll-up with a stated roll-up rule. Layout and mark-value changes stay separate commits; sync-coverage.mjs --check stays the gate.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lzstdlibmatrixrelease] Implement Phase 5 of tasks/osoftware/plan-lazily-stdlib-conformance-rollout.md: run the capability-driven all-peer/all-channel matrix, enforce CI and mutation gates, publish coverage evidence, and release bindings inside-out only after every claimed cell is green.
- [/] [#lzjsnpmtrustedpub] [operator-action] lazily-js repository workflow/OIDC/provenance is ready, but npm rejects publication until trusted-publisher setup is completed. NEXT: authenticate npm, register `@lazily-hub/lazily-js` for `lazily-hub/lazily-js` workflow `release.yml` with no environment and allowed action `npm publish`, verify with `npm trust list @lazily-hub/lazily-js`, then prove the next version tag.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily.done.md -->

<!-- completed work archived in tasks/software/lazily.done.md -->
<!-- /agent:done -->
eturn typed satisfied/timed-out/cancelled/disposed-or-unavailable outcomes; optionally prove keyed side-effect receipts while leaving receipt storage and transport policy outside the primitive. Test lost wakeups, already-satisfied state, revision races, cancellation, deadlines, disposal, and keyed receipts; document using the stdlib Timer as the deadline budget.
cate orphan line remains outside all agent-doc components at line 158. Please delete only the stray `- [ ] [#lzstdlibbarrier] ...` line in IDEA, save, and send `continue`. The completed item is already archived, so removing this duplicate is the only remaining closeout step.
