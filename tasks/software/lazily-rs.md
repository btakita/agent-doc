---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---



# lazily-rs: Reactive Signals Library

<!-- agent:status -->

- [x] Phase 1: Research & design
- [x] Phase 2: Core signal primitives
- [x] Phase 3: Derived/computed signals
- [x] Phase 4: Effect system
- [x] Phase 5: Testing & benchmarks
- [x] Phase 6: Documentation & publish
<!-- /agent:status -->

<!-- agent:exchange -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260605-030600.md`*

Compacted content:
- Archived 3 response topic(s): do [#xtwf] Rigorous A/B for #vd5v lock-free reads; do [#s0fc] Make the signaling endpoint consumable as a dependency; release + publish
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...
❯ Would it be possible to switch between the optimal low contention strategy and the optimal high contention strategy?
❯ >   2. The most valuable course-correction this session was #xtwf: the rigorous A/B caught that my #vd5v "perf win" claim
❯        was actually a low-contention regression / high-contention win, and the SPEC gate now reflects reality. Worth keeping
❯        in mind that 0.9.0 ships that tradeoff (a few % slower uncontended reads, big wins at 8+ cores) — documented in SPEC.
### Re: Switching between low- and high-contention read strategies — opus-4-8

> **Queue prompt:**
>
> release + publish


Yes — three ways, in increasing cost/complexity. The honest summary: a **compile-time switch** is cheap and clean; **runtime-adaptive** is possible but usually not worth it; the genuinely-better "best of both" is a **different storage representation** the SPEC already anticipates.

#### (a) Compile-time feature switch — cheap, recommended if you just want the choice
A `lockfree_reads` cargo feature selects the storage at build time: `arc_swap::ArcSwapOption` (high-contention) vs `parking_lot::RwLock` (low-contention). Zero runtime overhead either way; the read path is monomorphized to one strategy. The deployer picks per their workload. This is exactly the "feature-gated" option from the #vd5v decision — easy to add to 0.9.x so low-contention users can opt back to RwLock without losing the 8+ core win for everyone else.

#### (b) Runtime-adaptive switch — possible, rarely worth it
Two hard parts:
- **Migration.** Flipping a slot's storage mode while readers are in flight needs a barrier — you can't lock-free-ly convert RwLock-backed ↔ ArcSwap-backed storage. It *is* feasible to flip a slot's mode on a write (writers already hold the graph mutex), so migration could ride the write path.
- **Per-read cost.** Every read then branches on the current mode (`if lockfree { arcswap.load } else { rwlock.read }`) plus an atomic mode load. That branch lands on the *uncontended* path too — the one we're trying to keep fast — so you partially erode the benefit of *both* strategies. Add contention detection (sampled reader depth / contended-acquire counts) with hysteresis to avoid flapping, and per-slot state, and it's a lot of machinery that seldom recovers the full win of either pure path. Worth it only if contention is sharply bimodal per slot.

#### (c) The real "best of both" — inline `ErasedValue` + seqlock (SPEC's future direction)
The reason arc-swap costs anything uncontended is the `Arc` itself: refcount traffic + the `Arc<Arc<dyn Any>>` double-indirection I had to use. The optimistic-validation envelope (`cache_revision` read-before/read-after) is *already a seqlock*. If small `Copy`/`Clone` values were stored **inline** (the SPEC's anticipated `ErasedValue` replacing `Arc<dyn Any>`), a reader could copy the inline bytes and validate the sequence — **wait-free, no refcount, no per-reader atomic contention, no allocation** — which beats *both* RwLock and arc-swap across the contention spectrum. The catch: it's sound only for small/`Copy` (or cheaply-`Clone`, torn-read-validated) types; large/non-`Copy` values still need the arc-swap fallback. So it's a size-specialized fast path, not a universal swap — a real redesign, good for a 0.10/1.0 milestone.

#### Recommendation
- Want user choice now? I'll add the `lockfree_reads` feature (option a) — small, zero-cost, ships in a 0.9.x patch.
- Want the durable win? Spec out the `ErasedValue`/seqlock inline-small-value path (option c) as the next perf milestone; it subsumes the whole question.
- I'd skip (b) unless you have a concrete bimodal-contention workload that justifies the per-read branch.

Say which (or both a+c) and I'll implement / spec it. No code changed this turn — this is analysis.
❯ Can option a be a flag for a high-concurrency mode and a low concurrency mode? With the default being low concurrency mode.
❯ Would using c be a conditional on the size of the value?
### Re: (a) as a high/low-concurrency flag, default low — opus-4-8

Yes, with one constraint to design around: **cargo features are additive booleans**, so the clean shape is a single opt-in flag, default off = low-concurrency:

- **default (flag off) = low-concurrency mode** → `parking_lot::RwLock` reads (optimal uncontended). This is exactly your "default low concurrency."
- **`high_concurrency` flag on** → `arc_swap::ArcSwapOption` reads (optimal at 8+ cores).

You can't have a feature that *subtracts* default behavior (features must be additive — enabling one can't remove arc-swap if it were the default), so the toggle has to be "default RwLock, opt **into** lock-free," not "default lock-free, opt out." That happens to match what you want.

One honest consequence: **0.9.0 already shipped arc-swap as the *default*.** Flipping the default to RwLock (low-concurrency) is a default perf/dependency change — not a breaking API change, but it changes the out-of-the-box profile and drops `arc-swap` from the default build (it becomes `dep:arc-swap` behind the flag). That warrants a **0.10.0** minor bump, not a 0.9.x patch. Net win: low-concurrency users get the faster uncontended path *and* one fewer dependency by default; high-concurrency users add `features = ["high_concurrency"]`.

### Re: is (c) conditional on the value's size? — opus-4-8

Yes — and on one more property: **size AND copyability**.

The inline `ErasedValue` fast path applies when:
1. **`size_of::<T>() <= INLINE_CAP`** — the value fits a fixed inline buffer (small-buffer optimization, e.g. a `usize`/24-byte inline slot), and
2. **`T: Copy`** (or a sealed plain-old-data marker) — required for soundness, not just speed.

Why copyability, not just size: a seqlock read copies the bytes *without* synchronization and then validates the sequence (`cache_revision`). If a writer is mid-update, the reader can momentarily observe a **torn** value. For `Copy`/POD that's harmless — the torn bytes are discarded on the revision-mismatch retry, no `Drop`, no allocation, no interior pointers followed. For a non-`Copy` `Clone` type that owns heap/resources, cloning a torn value would dereference garbage pointers → UB. So the inline-seqlock path is gated to `Copy` within the size bound; **everything else falls back to the `Arc<dyn Any>` path** (RwLock or arc-swap per the (a) flag).

Mechanically: capture `(size, is_copy)` at slot creation (alongside the inline `TypeId` we already store), and the read branches on a per-node bool — a single load+branch, far cheaper than the `Arc` refcount it avoids. For the common reactive cases (numbers, small `Copy` structs) you get wait-free, allocation-free, contention-free reads; large or owning values keep today's behavior.

So the two compose cleanly: **(c) handles small `Copy` values optimally regardless of contention; the (a) flag governs only the large/non-`Copy` fallback** (RwLock vs arc-swap). That's the full "best of both," and it's a 0.10/1.0-scale change.

Still analysis — no code changed. If you want, I can: ship the (a) flag as 0.10.0 now (small), and/or open a spec'd plan for (c). Which?
❯ re a: I want this to be both a compile time decision and a runtime decision...where both paths are compiled and one mode or the other is selected in the runtime.
❯ The next release should also iclude c.

### Re: runtime-selectable read strategy + (c) for next release — opus-4-8

Implemented **phase 1** (runtime-selectable strategy) and planned the rest of the 0.10.0 milestone. Both your directives are captured.

**Phase 1 — runtime-selectable, both paths compiled (`#rdstrat1`)** — committed + pushed (`29dbf7e`):
- `ThreadSafeContext::with_read_strategy(ReadStrategy)` selects the mode at runtime; `new()`/`default()` = **`LowConcurrency`** (your requested default). `ReadStrategy::HighConcurrency` opts into the arc-swap wait-free read.
- The slot sidecar is now a `CachedReadStorage` enum (`Locked(RwLock)` | `LockFree(ArcSwapOption)`), variant chosen per the context strategy at slot creation. **Both paths are compiled in**; the cost is one per-read enum branch.
- Identical `cache_revision` validation envelope for both → cross-thread invalidation safety unchanged; the `thread_safe_loom` model still passes (same algorithm). New tests exercise both strategies; default suite green.
- This **flips the 0.9.0 default** (which shipped arc-swap) back to `LowConcurrency` — that default change is exactly why it's a 0.10.0 minor bump. Also fixed `BENCHMARKS.md`'s version line (the 0.9.0 release missed it; a version-tracking test caught it now).

**Phase 2 — (c) inline small-`Copy` seqlock (`#rdstrat2`)** — **planned, not yet built**. This is the genuinely delicate part: an inline byte buffer + even/odd seqlock storing small `Copy` values (size+`Copy` conditional, as we discussed), wait-free and allocation-free, optimal in *both* modes. It involves raw byte copies of type-erased values and **must have a Loom/Shuttle model before it lands** — I'm not rushing unsafe concurrency code into the same turn. Queued with the plan.

**Phase 3 — release 0.10.0 (`#rdstrat3`)** — queued; ships once phase 2 is in, gated on your confirm for the crates.io publish (per your "next release should also include c").

Plan with full design + phases: `tasks/software/plan-lazily-0.10-read-strategy.md`. Backlog now holds phases 2 and 3 in priority order. Want me to proceed with phase 2 (c) next — I'd start with the Loom model — or pause here?
<!-- agent:boundary:960aa220 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#vbyc] Phase 2 (#rdstrat2): inline small-Copy seqlock cached-read fast path for ThreadSafeContext (the SPEC's ErasedValue idea) — size+Copy conditional, needs a Loom/Shuttle model before landing. Plan: tasks/software/plan-lazily-0.10-read-strategy.md [recommended]
- [ ] [#9mhz] Phase 3 (#rdstrat3): release lazily 0.10.0 (runtime-selectable read strategy + inline seqlock; default flips to LowConcurrency) — gate on user confirm for crates.io publish. Plan: tasks/software/plan-lazily-0.10-read-strategy.md [recommended]
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#ipc1] [recommended] Design lazily-serde feature gate (trait bounds vs type-erased closures)
- [/] [#ipc2] [recommended] Design lazily-ipc snapshot + incremental update protocol
- [/] [#ipc3] [recommended] Investigate CRDT vs Raft trade-offs for lazily-distributed
- [/] [#39c5] #sig1 [recommended] Implement RemoteOp enum + per-peer permission allowlist for lazily-distributed remote access
- [/] [#yxjw] #sig2 [recommended] Build Cloudflare Worker signaling server for internet-scale peer discovery
- [/] [#vd5v] Future: leverage typed cache for lock-free cached reads in ThreadSafeContext (#lazilyperf38)
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
