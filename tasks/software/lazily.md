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
### Re: Fix the red test suites in the lazily implementations — opus-5

> **Queue prompt:** 📌 Fix the red test suites in the lazily implementations.

I audited all twelve lazily repositories. Exactly one suite was red, and it was not a defect: it was this document's own previous cycle, still running.

#### The red, and what it actually was

`lazily-zig` failed on Zig **0.15.2 and master** while passing on 0.16.0: `queue_family_conformance.zig`, "a derived node over a reader reruns exactly on invalidation (async)", at `try testing.expect(Derived.head_runs > head_runs)`.

I instrumented it, and it passed. I reverted the instrumentation, and it passed three more times. That is the signature of a tree being written underneath you, not a heisenbug, and it was: the previous cycle's `#lzqueuefamilyflavors` subagent was still working `lazily-zig` and had been for about forty minutes. It finished during this cycle, committed `c103070`, pushed, and CI run `30523011815` is green.

Current state of that repo, measured after it settled: **0.15.2 787/791** (4 version-gated `std.Io`/`mmap` skips), **0.16.0 849/849**, **master 849/849**, `make check` exit 0. Its own report records 15 mutation probes across the shared core and all six new shells, every one red then reverted green.

So there was no standing red suite to fix. That is the honest answer to the item, and the useful part of this cycle is what proving it turned up.

#### The measurement that nearly sent me at the wrong problem

`git diff --stat` through the RTK wrapper told me `src/lazily/queue_family_conformance.zig | 2 +-`. The real diff was **2062 lines**. Reading the tree from that number is what made me class `lazily-zig` as idle-with-a-broken-test rather than mid-flight, and I burned a detour chasing a five-line discrepancy between the line number Zig reported (2095) and where the assertion actually sat (2100). Those five lines were a comment block the other agent added between my test run and my read.

`rtk proxy git diff --stat` and `/usr/bin/git diff --stat` both reported 2062 correctly. This is the same false-pass class as the wrapper's per-file `diff` claiming "Files are identical" and bare `cargo check` returning a stale digest, but it had not been recorded for `--stat`. Memory updated.

#### Eleven repos green as committed

Each verified this cycle with an explicitly captured exit code, not a trusted summary line, and each with its coverage guard cross-checked against the runtime manifest rather than taken at its word.

| Repo | Evidence |
|---|---|
| rs | 701 `test result:` lines all ok, 5479 passed across ~35 feature sets; 129/134 fixtures |
| py | 650 passed; 130/134, manifest 130 unique |
| kt | 413 tests, 0 skipped, forced `--rerun-tasks` to defeat Gradle up-to-date; 131 fixtures across 23 required areas |
| js | 573 passed; 131/134 |
| dart | 621 passed; 128/134, `comm`-reconciled against the corpus |
| go | 326 top-level / 544 with subtests; also green under `-race`; 125/134 |
| cpp | 52/52 ctest, `NDEBUG`-stripping still intact so `assert()` is live; 112 fixtures |
| cs | 238 passed, **134/134 fixtures with an empty allowlist**; all six check stages ran |
| spec | 162 schema tests + all five stages; 12 coverage targets confirmed non-empty |
| formal | `lake build`, 52 jobs |
| react | 18 passed, and I had it mutation-probe its own gate 3-for-3 |

#### One real defect the greens were hiding

`lazily-js` shipped `src/transport.d.ts` importing `IpcValue` and using it in four type positions, but `IpcValue` is the const factory object; the union type is `IpcValueValue`. Four `TS2749` errors, so `BlobRouter.resolve`, `spillValue` and `resolveValue` had unusable signatures for any TypeScript consumer of the `/transport` subpath. It survived because **nothing in the repo typechecked the declarations it publishes**: no tsconfig, no `tsc` script, no CI step. A suite that never looks cannot report a failure.

Fixed and gated in `lazily-js 553d840`. All four positions were verified against `transport.js`'s `instanceof` dispatch before changing, and all four genuinely meant the union. The gate is a declaration-only tsconfig with `skipLibCheck: false` covering 43 of 43 `src/*.d.ts` (confirmed by `--listFiles`), wired into both `make check` and CI, plus a CI assertion that the program is non-empty, because an `include` matching nothing also exits 0. `typescript` was already a devDependency, so no lockfile churn.

Probed in both directions by exit code: with the defect restored, `npm run typecheck` exits 2 with the four `TS2749`s and `make check` exits 2 *before the tests run* (`grep -c "^ℹ tests"` = 0); reverted, exit 0; and the empty-program guard fires on an `include` that matches nothing. Final `make check` green, 573/573, CI run `30523255284` in progress at my single check.

#### Four gate gaps filed rather than folded in

None of these is currently red, and none is a stall dressed up as a finding: each is a specific way a green could be reporting less than it claims, each is actionable, and each belongs to a binding I would rather not reformat mid-audit.

- `#lzgoracegate` — go's gated library suite does not run under `-race`. It is green when I run it raced, so the hole is the gate. That is exactly where the v0.23.2 map data race fell through.
- `#lzcovallowlistrot` — the coverage guards cannot fail on a *stale excuse*, so py, go and dart are over-excusing 6, ~6 and 7 fixtures that are in fact being replayed. Ledger rot in the same understating direction `#lzcoverageaudit` just corrected one layer up.
- `#lzreactdtsgate` — lazily-react has the identical hole that hid the js defect, so it gets the identical gate.
- `#lzrsbenchbudget` — rs `benchmark-check` prints "NO BENCHMARK BUDGET WAS ENFORCED" and exits 0.

#### What `#lzqueuefamilyflavors` still owes

Since its implementation landed mid-audit, I rewrote that backlog item so the next cycle does not re-implement it. All three bindings have the code (`js 67a0013`, `cs f056dd8`, `zig c103070`). What remains is the ledger and the plumbing: coverage.json rows 17/18/20/21/23/24 still read `—` for the JS, Zig and C# columns, the twelve generated coverage tables need regenerating, `lazily-zig`'s README still claims every Zig row is shipped while its own table shows six absent, and three superproject gitlinks are unbumped.

I did not touch `lazily-zig` beyond reading it once its agent had committed, and I did not bump any gitlink, since that is that item's closeout rather than this one's.

### Re: do [#lzgoracegate] — opus-5

> **Queue prompt:**
>
> do [#lzgoracegate]


Done, `lazily-go 6ae14e5`. One line of Makefile, and the probe that proves it earns its place.

#### What was actually missing

Half of what the item claimed. CI already had a gating `race (cgo)` step at `.github/workflows/ci.yml:100` with no `continue-on-error`, so the remote side was never open. The hole was local only: `check: fmt-check vet build test test-interop-peer conformance-coverage` ran the library suite unraced, and the one race-instrumented thing in it was `test-interop-peer`, which exercises the FFI peer rather than the graph.

`race` now sits between `test` and `test-interop-peer`. Ordering is deliberate and commented: `test` truncates the conformance manifest and `race` never writes it, so the fixture union the coverage guard audits stays the one `test` recorded. No new toolchain requirement either, since `check` already needed cgo for the interop peer.

#### The probe took two attempts, and the first attempt is the interesting one

My first mutation was the obvious one: delete the lock from `Read` entirely. It went red, and it proved nothing about the new step. Plain `go test ./...` also failed, because unsynchronized map access trips Go's own runtime `concurrent map writes` panic in `TestAsyncEffectRerunsAcrossTwoWrites`. A defect the existing gate already catches cannot demonstrate that a new gate was needed.

So the probe had to be race-*only*: a graph read off the lock that corrupts nothing and fails no assertion.

```go
func Read[T any](t *ThreadSafeContext, fn func(ctx *Context) T) T {
	_ = t.ctx.cachedCount // MUTATION PROBE: graph read off the lock
	t.lock.Lock()
```

`cachedCount` is a plain `int` the cascade writes while readers read it. Result:

| gate | exit | signal |
|---|---|---|
| `go test ./...` | **0** | nothing — green, sees no defect |
| `make check` | **2** | **13 × `WARNING: DATA RACE`** |

That is the discrimination the item is about, in one table: the pre-existing gate is green on a defect the new one catches thirteen times. The first race pair reported is `markClean()` at `core.go:691` against a concurrent read, which is precisely the v0.23.2 shape — a *read* mutating graph state.

Reverted with an mtime bump, `make check` green: 125/134 fixtures, interop peer ok, `lazily-go: check OK`, exit 0.

#### One thing this does not fix

`race` runs `go test -race ./...` without `LAZILY_CONFORMANCE_MANIFEST`, so the raced pass records no fixture reads. That is correct for the manifest's sake, but it does mean the *conformance corpus* is replayed unraced while the unit suite is replayed both ways. If a fixture replay ever grows concurrency, that gap reopens. Not worth a second manifest today; worth knowing it is there.

CI run on `6ae14e5` was `in_progress` at my single check. Recorded, not polled.

### Re: do [#lzcovallowlistrot] — opus-5

Done across all nine bindings, eight commits. The item was right that the allowlists had rotted, but it undersold what the missing check was actually protecting.

#### What a stale excuse really costs

I filed this as tidiness: over-excusing makes the real gaps unreadable. That is true and it is the smaller half. The `lazily-rs` pass found the other half, and it changes the severity.

A stale excuse **re-arms the drift the guard exists to catch**. If a fixture is both replayed and allowlisted, and its replay later breaks, the excuse silently absorbs the failure — the guard reports an honestly-uncovered fixture instead of a regression. So the 19 stale entries across py, go and dart were 19 fixtures whose coverage guard could not have failed if their replays stopped working.

The `lazily-cpp` and `lazily-js` passes independently named why it was undetectable: a fixture that is opened *and* excused takes the `covered` branch and `continue`s before `KNOWN_UNCOVERED` is ever consulted. There was no code path that could observe the contradiction. Invisible by construction, not merely unchecked.

#### Results

| Binding | Stale found | Allowlist | Commit |
|---|---|---|---|
| py | **6** | 10 → 4 | `e62904d` |
| go | **6** | 15 → 9 | `868f8cc` |
| dart | **7** | 13 → 6 | `7f71fa8` |
| rs | 0 | 5 | `7fd89d5` |
| js | 0 | 3 | `569cf3b` |
| cpp | 0 | 10 | `bd045de` |
| zig | 0 | 24 | `727adec` |
| cs | 0 | 0 (empty) | `a5e56ae` |
| kt | n/a | different guard shape | no change |

py, go and dart now reconcile exactly: covered + excused = 134 in each. Before, py claimed 130 + 10 against a corpus of 134, which is arithmetically impossible and had been sitting there in the green output.

The three that rotted rotted on the **same six fixtures** — the four `collections/topiccell_*` and two `collections/workqueue_*` — plus `signaling/frames.json` in dart. That is the expected shape: the queue-family conformance work made those covered in every binding at once, and no allowlist was touched.

#### Two predictions of mine that the measurements killed

**zig was going to be the worst, and it was clean.** I reasoned that 24 excuses plus a large new queue-family runner in `c103070` meant obvious rot. Its allowlist is exactly the complement of the opened set — `comm` empty in both directions, and 110 + 24 = 134 with `missing=0` forces a partition. `c103070` stranded nothing because all eleven queue fixtures were *already* opened; what that commit raised was replay depth, each fixture now opened 5× across three flavors, not the manifest count. I read "large new runner" as new coverage when it was deeper coverage.

**cpp was going to be dirty, and it was clean too.** Its ten excuses are genuinely unread, including the five `reliable-sync` fixtures that `test_reliable_sync.cpp` hand-transcribes. That is the interesting part: the hand-transcription problem is real, and the ledger reports those as uncovered rather than claiming them. The excuse list is honest about the thing the binding is worst at.

#### On not deleting what the manifest merely opened

Every agent was told that an entry which is *opened* but not *replayed* must not be deleted, since that would mean the manifest records opens rather than replays — a worse defect than a stale excuse. Each traced its stale entries to a real replay before removing them: py through `TOPIC_FIXTURES`/`WORK_QUEUE_FIXTURES` parametrization with a `steps > 0 and matrices == steps` vacuity guard, go through `replayTopicFixture`/`replayWorkQueueFixture` with hard per-flavor step totals, dart through `_replayTopic`/`_replayWork` with the same. Nothing was deleted on the manifest's word alone.

go's is the cleanest case: it has a test that reads those fixtures via bare `os.ReadFile`, bypassing the recorder entirely, so it contributes no false opens. dart is cleanest structurally — its manifest deliberately excludes its one non-replaying read site, so a recorded open comes from a replaying suite by construction.

#### Verified rather than accepted

Eight agents, one repo each, no two in the same tree. I checked their claims rather than their reports: every commit SHA present at HEAD, every working tree clean apart from the two pre-existing edits I told them to leave (`lazily-py`'s `.claude/worktrees`, `lazily-zig`'s `AGENTS.md`), every script carrying the new check, every allowlist count matching what was claimed.

Then I ran the probe myself in `lazily-go` rather than trusting its report: injecting `agent-doc/delta_agent_doc_state.json` — a fixture in that binding's manifest — gives exit **1** with `KNOWN_UNCOVERED lists '...', but the suite DID open it. The excuse is stale`. Reverted, exit **0** at 125/134. Working tree clean after.

Every binding's new check reuses its own covered-check comparison verbatim (`grep -qxF "$known" <<< "$OPENED"`), which is the point: if the two spellings ever diverged, one fixture could read as covered by one branch and uncovered by the other in the same run.

#### kt is a different animal, and it already had the property

lazily-kt has no per-fixture excuse list — a `MIN_FIXTURES` floor plus `REQUIRED_AREAS`. So there was nothing to attach the check to, and the agent correctly changed nothing rather than inventing a mechanism.

Its excuse ledger lives in the test instead: `EXPECTED_SKIPS` in `ReactiveGraphConformanceTest.kt`, enforced by `assertEquals(EXPECTED_SKIPS, observed)` — **exact set equality**. A fixture that becomes replayable leaves the observed skip set and the equality fails. That is precisely the property the other eight allowlists lacked, arrived at by a different route. Probed: adding a provably-replayed fixture to `EXPECTED_SKIPS` fails the suite; restored, green.

What kt does have is a floor at 117 while 131 replay, so ~14 fixtures could go dark with the build green. It cannot conceal a *named* fixture and `REQUIRED_AREAS` still catches an area going dark, so it is slack rather than rot — but it is the same understating family. Filed as `#lzktfixturefloor` rather than folded in, because raising the floor is a policy call about how the number is maintained.

### Re: do [#lzktfixturefloor] — opus-5

> **Queue prompt:**
>
> do [#lzktfixturefloor]


Done, `lazily-kt 12832a5`. The item offered two fixes; the real one was available, so the floor is gone rather than raised.

#### What replaced it

lazily-kt's recorder already writes **corpus-relative** ids, which is exactly the attribution per-fixture accounting needs — so nothing structural was blocking the family shape. The guard now enumerates `../lazily-spec/conformance` and requires every fixture to be opened-or-excused, with all three rot directions from the start: an unopened fixture is an error, an excuse naming no corpus file is an error, and an excuse the suite DID open is an error. It also took lazily-go's self-guarding evidence channel, where every manifest id must resolve to a real file under the corpus root.

| | opened | total | excused |
|---|---|---|---|
| before | 131 | 134 | 0 — floor of 117, no ledger |
| after | **132** | 134 | 2 |

The two survivors are `reliable-sync/coalesce_bounds_outbox.json` and `reliable-sync/liveness_lease_eviction.json`, excused in every other binding for the same reason: no runner drives outbox-coalescing or lease-eviction anywhere.

#### The mutation that justifies the whole item

Drop `ingress_backpressure.json` from its runner list, so a fixture silently stops being replayed:

| gate | exit |
|---|---|
| the test suite itself | **0** — did not notice |
| old `MIN_FIXTURES` floor | 131 ≥ 117, would have stayed **green** |
| new guard | **1**, names `ingress/ingress_backpressure.json` |

That is the failure the floor was built to tolerate: 14 fixtures of headroom meant 14 fixtures could go dark with everything green. I re-ran the stale-excuse arm myself rather than trusting the report — injecting a manifest-present fixture gives exit 1 with `KNOWN_UNCOVERED lists '...', but the suite DID open it`; reverted, exit 0 at 132/134.

#### A gap that became a fix instead of an excuse

Turning the guard on immediately caught `delta_zero_copy_arrow.json` as never replayed. The lazy move was to excuse it — and kt would have been the *only* binding excusing it. But `ShmBlobRef` already implements the `backend` discriminator the fixture exists to validate, so the gap was closeable, and the agent closed it.

The part worth extracting is what it found on the way in. `assertAssertions` **silently ignores unknown keys**, so simply adding the fixture to the runner would have round-tripped it, reported it green, and never checked the one field it exists for. An explicit `first_op_payload_backend` arm was required. That is the vacuous-green shape this corpus exists to prevent, one level below where we have been looking: not "is the fixture replayed" but "having replayed it, is the assertion actually consumed."

That is a per-runner property and there is no reason to think kt is unique. Filed as `#lzassertunknownkeys` — a corpus key no binding implements would today be invisibly skipped in all nine, which is exactly the class of thing the ingress projection had to write into the spec as contract rather than leave as implementation detail.

#### `REQUIRED_AREAS` kept, retargeted

As a *manifest* check it is now strictly redundant, and the tempting move was to delete it. Asserted against the **corpus** it covers the one blind spot enumeration has: if the lazily-spec sibling is stale or partially cloned, whole areas vanish from the enumeration and the guard passes over a fraction of the real corpus while reporting a proud `N/N`. That silent-shrink case was the only legitimate job the floor did, and area names do it without a number that rots. It is now documented as a corpus-shape tripwire so nobody mistakes it for coverage.

#### Why the two ledgers stay separate

kt has a second ledger in the test — `EXPECTED_SKIPS`, enforced by exact set equality — and merging it into the script would be a category error in both directions:

- `KNOWN_UNCOVERED` answers **were the bytes read at all**.
- `EXPECTED_SKIPS` answers **having read them, did we assert on them** — the six merge-feed fixtures are read and parsed, so they count as OPENED, and their replay then stops at a named unsupported op (`merge_cell`, `drain_exhausted`).

Merge them and the script starts excusing fixtures it did read, while the test loses its op-level precision. `EXPECTED_SKIPS` was already rot-proof and is untouched.

With this, all nine bindings now account for the canonical corpus the same way, and every one of them can fail on a stale excuse. CI on `12832a5` was queued at the single check; recorded, not polled.
<!-- agent:boundary:f5131fb2 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- do [#lzassertunknownkeys]
- ~~do [#lzrsbenchbudget]~~
- ~~do [#lzktfixturefloor]~~
- ~~do [#lzgoracegate]~~
- ~~do [#lzcovallowlistrot]~~
- do [#vnmr]
- do [#lzreactdtsgate]
- ~~📌 Fix the red test suites in the lazily implementations.~~
- 📌 do [#lzegress]
- do [#lzqueuefamilyflavors]
- do [#lzwqfixtureconfig]
- do [#lzingressadapters]
- 📌 do [#kny1]
- 📌 Research https://github.com/Cysharp/R3 and if lazily can have an adapter. Benchmark lazily-cs vs R3.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzassertunknownkeys] A conformance runner that silently IGNORES unknown assertion keys reports a fixture as replayed while never checking the field the fixture exists for — a fixture can round-trip, pass, and prove nothing. Found in lazily-kt while closing #lzktfixturefloor: assertAssertions ignored delta_zero_copy_arrow.json's backend discriminator, so adding the fixture to the runner would have gone green without ever asserting the one thing it tests; an explicit first_op_payload_backend arm was needed. This is a per-runner property and almost certainly is NOT unique to kt. Audit every binding's fixture-assertion dispatch for the same silent-ignore path, then make an unconsumed or unrecognized assertion key a HARD ERROR rather than a no-op, so a corpus key no binding implements fails loudly instead of being invisibly skipped in all nine. Mutation-check per binding: introduce an assertion key the runner does not know and confirm the suite reddens rather than passing.
- [ ] [#lzreactdtsgate] lazily-react ships src/*.d.ts and a `types` export but its `make check` is only `node --check` syntax validation plus `node --test` (18 tests), so nothing typechecks the declarations it publishes. Same hole that let lazily-js ship four TS2749 errors in transport.d.ts undetected until this cycle. Port the gate landed in lazily-js 553d840: a declaration-only tsconfig (include src/**/*.d.ts, noEmit, strict, skipLibCheck:false, allowJs:false), an npm typecheck script wired into both `make check` and CI, plus CI's non-empty-program assertion, since a gate whose include matches nothing also exits 0. Mutation-check by breaking one declaration and confirming a non-zero exit before the tests run.
- [ ] [#vnmr] [ enforces benchmark budgets on a pruned target/: benchmark-check prints "SKIPPED: no benchmark evidence in this checkout (target/criterion does not exist)" plus "NO BENCHMARK BUDGET WAS ENFORCED BY THIS RUN. This is not a pass - it is the absence of a measurement" and exits 0. The banner is honest and the skip is documented, but a green `make check` still reads as budgets-verified to anyone not reading the log. Either make the skip a hard failure when a budget file exists but evidence does not, or have check depend on generating evidence first, so local closeout cannot claim a performance gate it did not run.
- [ ] [#lzqueuefamilyflavors] Ship the thread-safe and async queue-family flavors in lazily-js, lazily-zig and lazily-cs - the 18 cells that coverage.json rows 17/18/20/21/23/24 show absent. IMPLEMENTATION HAS LANDED IN ALL THREE BINDINGS: lazily-js 67a0013, lazily-cs f056dd8, lazily-zig c103070 (the last one mutation-checked with 15 defects across the shared core and all six new shells, green on 0.15.2 787/791, 0.16.0 849/849 and master 849/849, CI 30523011815 green). Do NOT re-implement. What REMAINS is the ledger and the plumbing: flip rows 17/18/20/21/23/24 for the JS, Zig and C# columns in lazily-spec/coverage.json, regenerate the twelve README/doc coverage tables via sync-coverage.mjs (--check is the gate), fix lazily-zig README line ~92 which claims every spec row is shipped for the Zig column while the generated table still shows those six as absent, and bump the superproject gitlinks for lazily-js, lazily-cs and lazily-zig. Verify each mark against that binding's own 3x3 ledger and replayed corpus before flipping it, not against the commit message.
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
