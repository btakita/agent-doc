# lazily #lzspecedgeindex — fan-out, disposal, and the family port

Continuation plan. Session of 2026-07-19 fixed an O(n²) dependency-edge defect
in `lazily-rs` and, chasing it with a load test, found four more. Five of eight
bindings still carry the original defect and six lack disposal entirely.

## State

### Where this stands (2026-07-19 close) — read this first

All 10 lazily repos are **synced with origin, clean, nothing unpushed**. Steps 1
and 2 below are **done**; step 3 is partly done; step 4 has not started.

| Step | Status |
|---|---|
| 1. Port to remaining bindings | **DONE** — all 8 audited, thresholds 32/64/128/160, no family constant |
| 2. Family disposal decision | **DONE** — `ctx.scope() -> TeardownScope`, `disarm()`; spec + Lean + 8 fixtures |
| 3. Flatten per-subscriber cost | substep 1 **DONE** (`3480cbb`); substep 2 blocked on a dedicated host; substep 3 gated behind 2 |
| 4. Release | **not started, and needs an explicit go** |

**Three open threads, none of which can proceed without a decision or a
different machine:**

1. **Observer-contract migration.** `lazily-spec` `71afb43` declares normative
   observer semantics and ships 5 fixtures. **Four bindings fail six clauses** —
   see the "Known divergences (migration required)" table in
   `lazily-spec/docs/reactive-graph.md`. The family currently ships a spec its
   own bindings violate. Two of the six (py and zig duplicate-registration) are
   genuinely breaking; dart/go unsubscribe-during-notify is arguably a bugfix.
   All bindings are 0.x. **Needs a go/no-go before starting.**

   Method note that inverts today's discipline: every observer fix on 2026-07-19
   was validated by tests passing *identically* against old and new, proving no
   behavior changed. These migrations change behavior on purpose, so their tests
   must **fail** against the old tree. The real acceptance criterion is the five
   `observer_*` fixtures flipping red to green.

2. **Huge pages (step 3 substep 2).** Timing-based, so it needs a dedicated
   host. Do not attempt on the workstation: on 2026-07-19 the same benchmark row
   measured 336 µs / 435 µs / 208 µs across three runs of identical code, and
   `dependency_fan_out/context/32` swung 1.6x across four runs.

3. **`lazily` vs `rmemo` benchmark.** The cleanest natural experiment for
   "eagerness is a scheduling policy, not a data structure": same primitive,
   opposite propagation policy (`rmemo`'s `set` drains eagerly by level;
   `lazily` marks dirty and recomputes on read). **Gated on step 3 finishing** —
   measuring now compares a finished implementation against a half-finished one.
   `rmemo`'s published numbers are in `ctx-core-dev/lib/rmemo/README.md`. Feeds
   the blog draft at
   `briantakita.me-dev/app/briantakita.me/post/content/2026-07-19-signal-is-not-a-primitive.md`.

**Standing measurement rules learned the hard way here** — apply them or repeat
the mistakes: prefer load-immune counters (scan iterations, `allocs/op`,
allocation counts) over wall time; establish a negative by forcing the path back
to naive behind a flag, never by observing a flat column; assume any
saturated-teardown arm is inert until an assertion proves otherwise (5 of 5
agents built a broken one first); and never dispatch two agents into the same
repo.

### Original state entry

Green: `make check` passes in `lazily-rs` and `lazily-spec`. Trees clean.

| repo | commits | note |
|---|---|---|
| `src/lazily-rs` | 15 | edge index, hasher, disposal, `TeardownScope` |
| `src/lazily-spec` | 2 | `#lzspecedgeindex` implementation note + disposal |
| `src/ctx-core-dev` | 6 | rmemo drain + error contract (separate thread, done) |

## Baselines — re-measure before claiming any improvement

Every number below was taken on this box (Ryzen 9 9950X3D, 186 GiB, CachyOS,
`rustc` stable) against a **clean `git worktree` at the parent commit**, not from
memory. Do the same. Single runs drift ±30% on the narrow cases; take medians.

```sh
cd src/lazily-rs
cargo run --release --example fanout_scaling            # narrow vs wide, n=500..4000
cargo run --release --example pubsub_load               # width ladder + churn soak
LAZILY_PUBSUB_MAX_WIDTH=10000000 cargo run --release --example pubsub_load
cargo run --release --features thread-safe --example pubsub_load_thread_safe
cargo run --release --example rc_handle_spike           # task 12 option B
cargo run --release --example teardown_scope_spike      # task 12 option C (shipped)
make benchmark-update                                   # regenerates BENCHMARKS.md
```

`pubsub_load` climbs a width ladder, projecting each rung's memory from the
previous one's measurement and refusing to build if it would not fit
(`LAZILY_PUBSUB_MEM_FLOOR_GIB`, default 16). 100M needs ~25 GB and ~5 minutes.

### Current numbers

Single-threaded `Context`, `pubsub_load`:

| width | build ns/sub | notify ns/sub | unread publish | bytes/sub |
|---:|---:|---:|---:|---:|
| 1,024 | 102 | 42.8 | 1,557 ns | 200 |
| 65,536 | 112 | 61.1 | 110 µs | 260 |
| 1,000,000 | 178 | 175.6 | 8.8 ms | 268 |
| 10,000,000 | 278 | 302.0 | 118 ms | 252 |
| 100,000,000 | 344 | 351.6 | 865 ms | 247 |

Per-subscriber memory is flat 260 → 247 B across a 1,500× range; that is what
makes the ladder's projection trustworthy. 100M = 23.5 GiB.

Session deltas, all measured against clean baselines:

| | before | after |
|---|---:|---:|
| wide fanout @ 4,000 | 47.9 ms | 2.25 ms (21×) |
| build @ 100M width | never finishes (~5×10¹⁵ ops) | 34 s |
| `ThreadSafeContext` build @ 262k | 50,799 ns/sub | 667 ns/sub (76×) |
| churn memory | 313.9 B/cycle, never reclaimed | 0.00 B/cycle |
| churn time | 23,419 ns/cycle | 126 ns/cycle (186×) |
| indexed registration | 83 ns | 45 ns |

`TeardownScope` vs disposing by hand, per node: build 156.9 → 72.5 ns @1024,
teardown 61.1 → 56.1 ns @65,536.

## Load test strategy — ≥1M fan-out

Every binding gets this before its port is called done. The whole reason five
defects hid for so long is that `benches/scale.rs` measures scale as *node
count* and fixes fan-out at 2, so width was never a variable.

### Why ≥1M is the floor

Two things only become visible there:

- **The quadratic becomes fatal rather than slow.** Building a width-N topic
  with an unconditional scan is ~N²/2 comparisons. At 4,096 that is 8M and
  looks like a 20% regression; at 1M it is 5×10¹¹ and takes minutes; at 100M it
  is 5×10¹⁵ and never returns. A ladder that stops at 65k cannot tell "slower"
  from "impossible".
- **The workload goes memory-bound.** On this box, notify jumps 60 → 151 ns/sub
  between 65k and 1M — exactly where the working set (15 MiB → 236 MiB) crosses
  the 128 MiB L3. Any optimisation measured only below that boundary is
  measuring a different machine than production.

### Method: climb, project, refuse

Never jump straight to the target. At each rung measure bytes/subscriber,
project the next rung from *that measurement*, and abort if the projection
would not leave a floor free. `pubsub_load` does this
(`LAZILY_PUBSUB_MEM_FLOOR_GIB`, default 16); it printed

```
stopping before width N: projected X GiB from the last rung's Y B/subscriber,
but only Z GiB available with a 16 GiB floor
```

which is what makes a 23.5 GiB run safe to start. This is haiven's method: it
proved 698 B/conn flat from 5M to 100M *before* attempting 100M. A wrong guess
at this scale takes the machine down.

Rung shape that worked: `32, 64, 96, 128, 129, 160, 256, 1k, 4k, 64k, 1M, 10M,
100M`. The cluster around the promote threshold is not decoration — it is what
exposed the 371 ns/sub demotion thrash at exactly threshold+1, invisible at any
other width.

### What to assert, not just print

A load test that only prints numbers gets skimmed. Each of these has an answer
that could embarrass the library:

| assertion | why |
|---|---|
| wide-vs-**narrow-control** build ns/sub at equal node count | linear build; catches any surviving O(n²) — see the correction below |
| bytes/sub flat within ~20% across the ladder | capacity planning is only possible if per-entity cost is flat — haiven's central finding |
| churn returns to baseline RSS (≈0 B/cycle) | catches the disposal leak; needs disposal to exist first |
| every survivor observes the final publish | a stale index entry shows up as a missed update, not a crash |
| notify unchanged by an edge-index change | propagation walks bare ids; if it moves, the model is wrong |

**Correction, 2026-07-19 (from the zig port).** The first assertion used to read
"build ns/sub grows < 2× from 1k → 1M". **That form is unsound** and would have
caused a wrong conclusion. On a *correctly fixed* engine zig measures **2.63×**
over that range — while `notify`, which does no dedup work whatsoever, grows
**6.8×** over the same range. Every phase picks up a memory-hierarchy slope once
the working set outgrows cache, so the absolute ratio conflates DRAM latency
with algorithmic complexity. Asserting it risks "fixing" a cache curve, or
failing a correct implementation for a reason that has nothing to do with edges.

Assert a **narrow-fan-out control at the same node count** instead: build the
same number of nodes at fan-out 2 and compare per-subscriber cost wide-vs-narrow
at each rung. That holds allocation and cache behavior fixed and varies only
width, which is the variable under test. Calibration from zig: control ratio
1.0–1.4 up to 262k, saturating at 5.4 at 100M, with 1M→10M costing 1.8× for 10×
the width — against an unfixed baseline of 148× at 65k and 597× at 262k. The
signal is unmistakable when measured against the right control.

Note this is the *sixth* case in this workstream where a conclusion reached by
inspection was wrong and one reached by measurement held.

### Per-binding targets

1M is the common floor. Above it is per-binding and must be *measured*, not
assumed — Rust reaches 100M at 247 B/subscriber in 23.5 GiB, and nothing else
should be expected to match that without evidence.

| binding | expect | watch for |
|---|---|---|
| rs, cpp, zig | 10M–100M | flat bytes/sub is the headline claim |
| go | 10M | GC pause during the ladder; measure with GC stats on |
| kt | 1M–10M | JVM needs an explicit heap flag; object header inflates bytes/sub |
| py | 1M | per-object overhead is large; 10M may be impractical and that is a fine result to report |
| js, dart | 1M–10M | V8/Dart old-space cap; `--max-old-space-size` needed past ~1M |

A binding that cannot reach 1M should say so with the measured ceiling and the
limiting factor. That is a real finding, not a failure.

### Out of scope

Do not port haiven's shed, backpressure, or latency-percentile suites. A
reactive graph has no queue, no admission control and no client — under load
`get` simply takes longer. Deliveries/sec has no analogue either; criterion
measures per-operation cost better than a load generator would.

These runs are minutes long and allocate tens of GiB. Keep them **manual and
on-demand**, like haiven's, not in CI. `make benchmark-check` stays the CI gate.

## What was wrong, and what it cost to find

Worth reading before touching the other bindings, because every one of these is
a trap the ports will hit.

1. **The threshold is not portable.** 32 was said to come from a JS
   measurement. In Rust the crossover measured near 170 with SipHash and near 40
   once ids were hashed with splitmix64 — a 4× shift from changing the hash
   function alone. Shipping 32 made widths 33–170 *slower than before the fix*.
   Now 32 again, for a different reason.

   **2026-07-19: the JS provenance does not survive measurement either.** A real
   in-binding crossover measurement gives **160** for `lazily-js`
   (`Map<edgeId, position>`; a bare `Set` crosses near 96 but only gives
   membership, and the recompute path removes an edge every recompute). At width
   32 the hash index measured **2.3× slower than the scan** in JS. So 32 was
   wrong for the language it was supposedly measured in. Treat every inherited
   constant in this family as folklore until a binding re-measures it.
2. **Demotion thrashes without hysteresis.** A dependent list oscillates by one
   on every recompute, so a single shared promote/demote boundary rebuilds the
   index every time: 371 ns/sub at the boundary against 89 either side.
3. **`ThreadSafeContext` has its own everything** — its own `edge_insert`, its
   own `HybridSet`/`HybridMap`. It is not a default feature, so `cargo build`
   and `cargo test` never compile it. Two separate linear scans there, and the
   second was only visible after fixing the first.
4. **Nothing could be disposed.** No `dispose_slot`/`dispose_cell` existed;
   `SlotHandle` is `Copy`, so dropping every handle reclaimed nothing.
5. **Disposal dropped nodes under the `RefCell` borrow**, so any capture whose
   `Drop` re-entered the context panicked. Live bug, reachable from safe code.

Findings 3–5 were invisible to the existing suite. `benches/scale.rs` runs 2M
nodes but fixes fan-out at 2, so it structurally could not see any of this.

**Method note:** every conclusion reached by inspection this session was wrong
at least once (the boxed field's cost, the `len()` explanation, the demote
experiment, the threshold twice, "per-node RAII is impossible"). Every
conclusion reached by bisection or a clean-baseline measurement held. Bisect.

## Next, in order

### 1. Port to the remaining bindings (task #4) — subagent fan-out

Five independent repos, no shared files. Good parallel work.

- `lazily-rs` — done, the reference
- **`lazily-kt` — DONE 2026-07-19 (`e9d1aaf`), threshold 32.** Build ns/sub grew
  **154×** from 1k→262k before, **1.43×** from 1k→10M after. Reached 10M,
  bytes/sub flat ~250 B (1.02× spread) despite JVM headers, `-Plazily.loadHeap=24g`.
  Measured crossover is **16**, not Rust's 40 — Kotlin's scan walks a boxed
  `ArrayList<Int>` while the index probes a raw `IntArray`; 32 was chosen over 16
  only because the columns between them differ by less than run-to-run spread.
  Demotion hazard **21.5×** at exactly threshold+1 (Rust measured 4×); demote
  sits at 24. `ThreadSafeContext` had the same defect and worse — linear scan on
  both add *and* remove. Recycled-id hazard is structurally absent here (the
  index is a field of the node, not a side table), pinned with a test.
- **`lazily-zig` — DONE 2026-07-19 (`4766a3c`), threshold 64.** Both defects
  fixed, and **measured not to compose**: with the registration fix alone and
  `remove` forced back to the scan, destroy returns to baseline. Registration
  3.8× → 457× (1k → 262k); `EdgeSet.remove` 3.4× → 279×. Reached **100M**, ~31 GB,
  ~7 min, bytes/sub flat within **2.9% from 4k to 100M**. No hysteresis needed
  because there is no demotion path at all. Two finds unreachable by inspection:
  draining the spill to empty silently reverted to the inline path while leaving
  a live index over a stale table; and every pre-existing `EdgeSet` test capped
  at `inline_cap + 1`, so the spilled path carrying the defect was **entirely
  untested**.
- **`lazily-cpp` — DONE 2026-07-19 (`d266a19`), threshold 32, demote 8.**
  Build **64.6×** and notify **867×** at 65,536. Reached **100M**, 265.9 B/sub in
  ~24.8 GiB, 2m39s, bytes/sub flat within 7.2%; the refuse path was exercised
  (correctly declined 1B at a projected 277.7 GiB). Hysteresis required —
  145.5 ns/sub at width 96 against 25.8 at 98 with promote == demote. **The
  thread-safe header needed no separate fix**, unlike `lazily-rs`: it is a lock
  wrapper delegating to one embedded `Context`, verified by measurement
  (36,115 → 44.4 ns/sub at 65k) rather than by reading it.

  Accepted tradeoff, documented rather than half-fixed: a stable **20% regression
  on `high_fan_out/512`**, the one benchmark iterating a wide list without
  touching an edge, caused by +16 bytes of node size. A variant keeping
  `dependencies` a plain vector removes the regression and is faster elsewhere,
  but leaves wide *fan-in* fully quadratic (25,126 vs 55.6 ns/dep at N=262,144).
  Fan-in degree is degree.
- **`lazily-dart` — DONE 2026-07-19 (`93da7c8`), threshold 128.** Build **56×**
  at 65,536 and **209×** at 262,144; the wide-vs-control ratio goes 325× → 0.56×
  at 65k. Reached **10M** with `--old_gen_heap_size=24000` (~3.6 GB live),
  bytes/sub flat 333–365 across 262k→10M; the refusal path never fired, so 10M
  is the top rung rather than a ceiling. Like zig, it **also fixed the removal
  half** — the index stores each dependent's *position*, so `_removeDependent`
  swap-removes and the O(n²) teardown goes with the same structure.

  Threshold reasoning is the most transferable result of the five, and is now in
  the spec: the pure crossover measured **60 under AOT and 96 under JIT** (a 1.6×
  spread from compilation mode alone), but that is the wrong number for a hybrid
  — see the spec note. **It originally shipped 96 and that was wrong**, caught
  only by running a high-repetition narrow guard against the baseline.

  Hysteresis kept (demote 32) but **never empirically forced**: Dart clears
  `_dependents` wholesale during cascade rather than oscillating by one, so the
  shared-boundary mode does not arise on that path.

#### A sixth defect: a second quadratic behind the first (`lazily-cpp`)

Finding 3 in the list below — "two separate linear scans, and the second was only
visible after fixing the first" — **repeated in a different place**, and this one
is not an edge-set problem at all. In `lazily-cpp`, `flush_effects` popped an
effect and unscheduled it, then `run_effect` scanned the entire remaining pending
queue for an id **provably not in it**: O(W²) per publish. The existing
`effect_scheduled_` bitset answers it in O(1).

It matters because the edge index alone did not fix wide notify — without this,
notify still grew 266 → 976 ns/sub from 1k → 4k. A binding could ship the edge
index, measure a large build win, and still be quadratic on publish.

**Audited 2026-07-19 — it is in the reference implementation too, and it moves.**

**All eight bindings audited 2026-07-19. Four carried it, four did not.**

| binding | notify path | teardown path | measured |
|---|---|---|---|
| rs `1acd202` + `2b98ca6` | **fixed** (both contexts) | **fixed** (tombstones) | 222× notify; 286× / 76× teardown |
| cpp `d266a19` + `ba9ba34` | **fixed** | **fixed** (position index) | 211.6× teardown |
| kt `510a874` | clean | **fixed** | 10,677× @262k |
| js `148e83e` | clean | **fixed** | 8.8× @65k |
| dart `233e1d8` | absent | absent | ~90× margin, nothing found |
| zig `70cf3e5` | absent | absent | 113× / 2230× margins |
| go, py | not audited (already hash-set edges) | | |

Note the split: rs and cpp had it on **both** paths, kt and js on the **teardown
path only**, and no binding had it on notify alone. A notify-only audit would
have found two of the four.

Three things this teaches about hunting it:

- **It is not always on the notify path.** cpp and rs had it in `run_effect`;
  kt had it in `disposeEffect`, which is why kt saw it in the teardown column and
  not in notify. Look for *any* scan of the pending/scheduled effect collection,
  on either path.
- **A binding can have it on BOTH paths, and fixing one leaves the other.**
  Confirmed and fixed in cpp (`ba9ba34`): after `d266a19` guarded
  `remove_pending_effect`, `dispose_effect` still scanned unguarded —
  **211.6× at width 65,536**. rs has the identical asymmetry
  (`context.rs:1362` bare `pending_effects.retain(...)` vs the guarded
  `remove_pending_effect` at `:1570`); being re-measured. **Audit every call
  site that touches the pending queue, not the first one you find.**

- **Count iterations, don't time them.** cpp established the defect exactly by
  instrumenting the scan: 134,225,920 iterations against W²/2 = 134,217,728 at
  width 16,384. That is load-immune, unambiguous, and needs no interleaving or
  median-of-N — strictly better evidence than a timing ratio when what you want
  to prove is asymptotic. Use it wherever the suspect path can be counted.

**Two prescriptions in an earlier revision of this document were wrong, and
measurement caught both:**

1. *"kt's empty-collection trap applies here."* It does not, in cpp. kt's
   `ArrayDeque.indexOf` scanned its backing array when empty; cpp's
   `deque_erase` loops `i < dq.size()`, so an empty deque skips the body
   entirely — measured at 0 iterations, max depth 0, for both plain and batched
   teardown. Only dispose-*during*-flush populates the queue. The original
   "only bites while effects are queued" reading was correct and was overruled
   on a bad analogy.
2. *"Guard it with the existing bitset, as `remove_pending_effect` does."*
   **Wrong in both cpp and rs, for the same reason.** The bitset answers "is
   this queued" in O(1) — but the biting shape is *precisely* mass teardown
   during a flush, when **every disposed effect IS queued**, so the guard
   returns true and the scan runs anyway. cpp measured iterations **unchanged**
   at 134,225,920 with the guard applied and nothing else.

   A flag guard can only help the not-queued case, which in Rust was already
   free (`Vec::retain` walks `len`, not capacity — unlike kt's `ArrayDeque`).
   So the guard is worth keeping as a cheap early-out, but it is not the fix.

   Two working fixes, both O(1), pick per binding:
   - **Position index** (cpp `ba9ba34`): id → absolute position
     (`base_ + offset`, because `pop_front` shifts), erase = swap-with-back.
     Same shape as `EdgeSet`. Build it **lazily on first erase** — an eager
     build on `push_back` regressed wide notify ~1.5× (28–38 → 45–57 ns/sub)
     for nothing, since a publish never calls erase at all.
   - **Tombstones** (rs `2b98ca6`): stop removing eagerly. Disposal clears the
     flag in O(1) and leaves the entry; the pop discards entries whose flag is
     clear. This also preserves the id-recycling safety the eager scan existed
     for — a recycled-and-rescheduled id has the tombstone claim the flag and
     run once, and the second entry finds the flag clear and is dropped.

**Five of five agents built a broken saturated-teardown arm on the first
attempt**, each for a different reason:

- dart — `batch()` defers the cascade to batch exit, so the queue stayed empty;
- zig — `Slot.emitChange` drains the queue itself;
- cpp — the invalidation walk uses a stack, so a disposer registered *first* is
  scheduled *last* and ran after the queue had drained (it must be registered
  last to run first);
- rs — creation order does not determine flush order at all, because the
  dependents list is swap-removed and reordered;
- js — the saturated arm was inert *and* the naive arm was broken: dropping the
  drain reset left `pendingHead` one past the array, so pushes landed behind the
  head and effects **silently stopped running**.

The first four produced a **flat column that reads as a clean negative**. js's
naive arm produced the opposite failure — a confident **186× margin measured on
a graph that had stopped working**. So the trap cuts both ways: a broken arm can
manufacture a false positive as easily as a false negative.

Guard both directions. js added a **run-count parity assertion across arms**
(every arm must execute the same number of effect runs) plus
`verifyCascadeSaturated`, which checks peak queue depth and throws before any
timing is trusted. Parity is the cheap check that catches "this arm isn't doing
the same work"; depth is the one that catches "this arm never touches the path."

**Instrument the queue depth the path actually sees, and assert on it.** Do not
infer saturation from how the harness was written. zig asserted
`pending.len == width` and failed loudly with `error.QueueNotSaturated`; rs
added `--cfg audit_probe` recording observed depth and got `queue max 0` at
every rung, which is what exposed its false negative. rs kept the probe
committed so the failure cannot recur silently — do the same.

A corollary worth stating plainly: **a low forced-naive detection margin is
evidence about your harness, not about the code.** Genuine negatives here came
in at 67×, 90×, 113×, 129.7×, 755× and 2230×. Anything in the low single digits
means the arm probably never touches the path.
- **"The queue is empty, so the scan is free" is false.** In kt the pending
  deque is empty at teardown, yet `kotlin.collections.ArrayDeque.indexOf`
  computes `tail = (head + size) % capacity` and takes its wraparound branch when
  `head >= tail` — which an empty deque satisfies, since `tail == head`. It then
  scans the whole backing array, which never shrinks. So after one wide flush,
  every later dispose scanned W slots for an id that could not be there. No
  amount of reading the call site would surface that; kt found it by bisection
  (`SmallEdgeList.remove` flat at 24.2→13.8 ns while `disposeEffect` grew
  961→43,434 ns over the same widths, locating the cost outside the edge list).

Existing harnesses cannot see this defect: `pubsub_load` measures pull-based
reads through computed slots, and effects are not on that route. rs added
`examples/edge_audit.rs`, which holds total work fixed (65,536 effects) and
varies only fan-out width, so every rung does identical work.

This is the second and third time "fixing scan #1 reveals scan #2" has appeared
here. Treat one fix per path as insufficient evidence by default.
- **`lazily-js` — DONE 2026-07-19 (`fbc3223`), threshold 160.** Measured against
  a clean baseline, node v26.4.0, JIT warmed, medians of 7:

  | width | build ns/sub | notify ns/sub |
  |---:|---|---|
  | 1,024 | 431 → 447 (0.96×) | 871 → 541 (1.6×) |
  | 4,096 | 551 → 287 (1.9×) | 1,795 → 258 (7.0×) |
  | 65,536 | 6,521 → 271 (24.1×) | 19,379 → 269 (72.0×) |
  | 262,144 | 26,338 → 646 (40.7×) | 108,405 → 417 (260×) |

  Reached 1M with every assertion passing (build 1k→1M = 1.31×, bytes/sub flat
  to 1.8%). Needs `--expose-gc --max-old-space-size=12288`; `--expose-gc` is
  mandatory or bytes/sub is meaningless. Demotion hazard reproduced in JS:
  **7.67× steady-state cost at exactly width 160, ~1.00× at every other rung** —
  shipped variant does not demote at all.

  **The "11.4× at n=4000" claim in an earlier revision of this document was
  unsubstantiated.** The number appeared exactly once in the whole superproject
  — this file — with no prototype, harness, baseline, or date; no such code
  exists in git history, stashes, branches, or dangling objects. By this
  document's own standard it never met the bar. The table above replaces it.
- `lazily-go`, `lazily-py` — already hash-set edges, no change needed

#### Unrelated blocker found by two ports independently: `crdt_sync_frames.json` drift

`lazily-kt` and `lazily-js` both fail conformance on the **same spec fixture**,
and neither failure is caused by the edge index — both reproduce on their clean
parent commits. `lazily-spec`'s `conformance/distributed/crdt_sync_frames.json`
moved in `3c28120` (`#lzperfaudit`) and the bindings did not follow:

- spec's fixture is **2.6K**; `lazily-kt`'s bundled copy at
  `src/test/resources/conformance/distributed/crdt_sync_frames.json` is **2.1K**
- kt fails in `IpcMessage.fromJson`; js fails the round-trip assertion
  `crdt_sync_frontier_suppressed`

It hid because `lazily-kt` CI has **no `lazily-spec` sibling**, so CI replays the
stale bundled copy and stays green — the failure only appears locally, where the
sibling is present. That is the same blind spot that let the edge-index defect
survive: the check that would have caught it does not run where it matters.

Worth a dedicated pass before the release step; it will otherwise surface as a
release-blocking conformance failure across several bindings at once. Check
go/py/dart/cpp for the same staleness rather than assuming it is only these two.

Brief each agent with `lazily-spec/docs/reactive-graph.md` (the
`#lzspecedgeindex` implementation note), not with lazily-rs's constants. Require
a measured before/after from a clean baseline per binding. Reject any
performance claim without one — inventing plausible numbers is the likely
failure mode.

`lazily-zig` has an extra defect: `EdgeSet.remove`
(`src/lazily/context.zig:558` — the path above omitted the `lazily/`) is a
linear scan called during cascade, an O(in-degree) cost per edge on wide fan-in.

**Corrected 2026-07-19: "that no other binding has" was wrong.** `lazily-dart`
had the same quadratic on removal and fixed it in the same change, by storing
each dependent's *position* in the index so `_removeDependent` swap-removes.
zig measured its removal fix at 3.4× → 279× (1k → 262k) and, importantly,
**proved the two fixes do not compose**: with the registration fix alone and
`remove` forced back to the scan, destroy returns to baseline. Registration and
removal are separate defects requiring separate fixes.

Treat removal as part of the standard port, not a zig special case.

**Audited 2026-07-19: absent in rs and kt, and both proved it properly.** A flat
column alone would not establish absence — it is equally consistent with a
harness that never touches the path. Both forced the lookup back to a linear
scan behind a declared build flag (rs `--cfg naive_edge_remove`, kt
`EDGE_FORCE_SCAN_REMOVE`), keeping index maintenance identical so only the scan
differed, and ran both arms interleaved:

- rs: naive arm 38.6 → 2,964.2 ns/sub across the ladder (67× at the widest rung);
  shipped path flat at ~44.
- kt: teardown drifts **755×** wide-vs-narrow with the scan restored, 1.59×
  without — three orders of magnitude of detection margin, and nothing detected.

Still unaudited for removal: **cpp and js.**

Adopt this force-it-back-to-naive pattern as the default way to establish a
negative. "The column is flat" is not evidence that a defect is absent; it is
evidence that *either* the defect is absent *or* the harness is blind.

### 2. Family disposal decision (task #11) — needs a human

Only `lazily-js` and `lazily-rs` can dispose slots/cells. go/py/kt cannot, and
GC does not save them: `lazily-py/src/lazily/cell.py:28` holds
`_parents: set[Slot]` and `lazily-go/core.go:54` holds
`dependents map[reactiveNode]struct{}` — both strong, so a long-lived topic
keeps every slot that ever read it. cpp/zig/dart unchecked.

Three mechanisms, all spiked and measured in `lazily-rs` (see task #12):
explicit disposal, `TeardownScope` scopes, and `Rc`/`Weak` handles. Recommended:
groups + explicit, both on `Copy` handles. Only `Rc`/`Weak` closes the
cross-owner hazard, and it costs `Copy` — a `.clone()` at every fan-out capture
site, or a gtk-rs-style `clone!` macro. The GC'd bindings may want weak
back-edges instead, which is what `ctx-core`'s rmemo does and needs no API at
all. Decide before writing the same thing five times.

### 3. Flatten per-subscriber cost (task #10) — **UNGATED 2026-07-19**, in order

Steps 1 and 2 both landed, so this is now the active work.

Growth past 1M is memory-bound, not algorithmic: notify jumps 60 → 151 ns/sub
exactly where the working set crosses this box's 128 MiB L3.

**Measure structure, not time, wherever the question allows it.** Step 1 below
is an allocation-layout question — counts, sizes, and how many distinct
allocations one logical node touch spans. All of that is load-immune, which
matters because this workstation is in normal use and today every timing-based
conclusion drawn on it was wrong at least once. `lazily-cpp` settled a quadratic
by counting scan iterations (134,225,920 against W²/2 = 134,217,728) and
`lazily-go` settled an allocation defect with `allocs/op`; neither needed a
quiet box. Step 2 is the one that genuinely needs timing, and therefore a
dedicated host.

1. ~~Measure where 247 B/node goes.~~ **DONE 2026-07-19 (`3480cbb`). The
   hypothesis was WRONG.**

   It guessed 3–4 scattered allocations (arena entry, `Rc<dyn Fn>` closure,
   `Arc<dyn Any>` value). Measured: **1.000 heap allocations per node**, at
   every width from 1,024 to 1,000,000. The arena is one shared
   `Vec<Option<Node>>` and nodes are contiguous, not individually allocated. The
   `Rc<ComputeFn>` box (32 B) is the *only* per-node block. The `Arc<dyn Any>`
   value box **does not exist for this workload** — `AnyValue::Inline` keeps a
   `u64` in the node. Isolated by re-running with a `String` value, which does
   cost `+48 B/sub, +2 blocks/sub`.

   Breakdown at width 1,000,000, summing exactly to the counted 243.81 B/sub:

   | B/sub | what |
   |---:|---|
   | 167.77 | arena `Vec<Option<Node>>` (160 B payload + 7.77 B slack) |
   | 32.00 | compute-closure `Rc` box |
   | 35.65 | topic `EdgeIndex` (hashbrown, 17 B/entry incl. ctrl byte) |
   | 8.39 | topic `dependents` EdgeVec |
   | **243.81** | RSS 287.17, so 43.4 B/sub is allocator metadata + page rounding |

   **The real cache story is better than the hypothesis.** Invalidation
   dereferences exactly one allocation and never touches the closure box — but
   the fields it needs are split inside the node: `dependents` @32 (line 0),
   `dirty` @152 and `force_recompute` @153 (line 2). So marking sets **2 bits
   per node while striding 160 B and touching 2 distinct cache lines**. That is
   the memory-bound story behind notify 60 → 151 ns/sub, and it is a layout
   problem rather than an allocation-scatter problem.

   Layout notes worth keeping: `Node` and `Option<Node>` are both 160 B (the
   discriminant and the `Option` niche pack into `SlotNode`'s tail padding, so
   the largest-variant tax is zero for slots — but `CellNode` is 80 B paying
   160). `AnyValue` is 48 B to hold at most 24 usable inline bytes.

   **Correction to the "flat bytes/sub" claim above.** Payload is flat at
   **217 B/sub** past the promote threshold (160 node + 32 closure + 8 edge +
   17 index), confirmed by a marginal cost of 394.00 B/sub and 1.000 blocks/sub
   across 1024→4096→65536. The 200 → 247 drift in the ladder is **`Vec` capacity
   slack**, sawtoothed against power-of-two doubling, not per-node growth. At
   width 65,536 the arena costs **320 B/sub** — 160 B payload against 160 B
   slack, the pathological point — while at 1,000,000 it costs 167.77 B/sub at
   4.9% slack. A projection taken from a rung sitting just past a doubling will
   therefore over-estimate the next rung by up to ~2x. The ladder's
   climb-project-refuse method is still sound, and erring high is the safe
   direction, but the "flat per-subscriber cost" framing hides a sawtooth.
2. Test transparent huge pages. 23.6 GiB in 4 KiB pages is ~5.9M PTEs.
3. Only then consider a dense flag arena for invalidation. Marking needs one bit
   but pays a full node touch; `lazily-js` already does this
   (`#lzjsarenanodes`). Would target the 865 ms unread publish at 100M.

   **Step 1 strengthened this case.** A 1M-wide mark currently streams 160 MB of
   arena to flip 2 bits per node; the equivalent bitset is 125 KB, which fits in
   L2. That is a ~1280x reduction in bytes touched and removes the L3 crossing
   from the marking path entirely.

   **But a pure flag arena is not sufficient for the general case**, and this is
   what step 2 must settle first: the mark walk also reads `dependents` (offset
   32) in order to recurse. The bitset only pays off if the walk can decide
   whether to recurse *without* touching the node. In the pub/sub shape it can,
   because subscribers have no dependents — but a general graph needs a dense
   degree / has-dependents side array, or a dense edge structure alongside the
   flags. Sizing that is the open question.

   **Cheapest thing to validate first, in isolation:** the repeat *unread*
   publish. Its early-out reads `slot.dirty` at offset 152, so it fetches a full
   64 B line per node purely to test one bit. No recursion question, no edge
   walk — the cleanest possible demonstration that a bitset helps, and it is
   exactly the 865 ms path.

Do not start 3 before 1 and 2.

### 4. Release: lazily family + ctx-core (tasks #5, #6)

Do not start until 1 and 2 land — publishing a partial family means bindings
disagreeing about `#lzspecedgeindex`.

#### Blockers, resolve first

- **`npm whoami` returns E401.** The stored `~/.npmrc` token is dead, so
  `publish--force` fails on all 192 `ctx-core` packages. Needs a human login.
  That login is the moment to switch to an **org-scoped granular token** rather
  than restoring a classic one — npm is deprecating 2FA-bypass tokens
  (`gh.io/npm-gat-bypass2fa-deprecation`).
- **`BENCHMARKS.md` must be regenerated** (`make benchmark-update` in
  `lazily-rs`) or `make check` fails on `benchmark-check`. Its earlier red was
  stale criterion data, not a regression — do not misread it as one.
- **Version bumps + `VERSIONS.md`** before any tag. `lazily-py` also needs
  `pyproject.toml`.

#### Publish order

Spec first, because the bindings replay its conformance fixtures.

1. `lazily-spec` — no registry; tag only. Version field is `"0"`; protocol
   versioning is per-message, not per-repo.
2. `lazily-formal` — no registry; tag only.
3. `lazily-rs` — crates.io. **Currently 0.45.0.**
4. Bindings in any order — kt, py, js, go, dart, cpp, zig, react.
5. `ctx-core` last: it is a separate dependency graph and does not consume
   lazily.

#### Per-target mechanics

| target | registry | how | auth |
|---|---|---|---|
| `lazily-rs` | crates.io | **local `cargo publish`** — only `ci.yml`, `mdbook.yml`, `regressions.yml` exist; no release workflow | **interactive** — use `CARGO_REGISTRY_TOKEN="$(passage show btak/CARGO_REGISTRY_TOKEN)"`; `credentials.toml` is stale and 403s |
| `lazily-js` | npm `@lazily-hub/lazily-js` | CI `release.yml`, OIDC + `--provenance` | none, *if* the Trusted Publisher is registered on the package — otherwise E404 on PUT |
| `lazily-py` | PyPI | CI `wheels.yml` on tag | none — `PYPI_PUBLISH_TOKEN` secret (`passage btak/PYPI_PUBLISH_TOKEN`) |
| `lazily-kt` | GitHub Packages | CI `release.yml` on `v*` tag, `GITHUB_TOKEN` | none |
| `lazily-go` | Go proxy | tag push, proxy pulls | none |
| `lazily-dart` | pub.dev | **local `dart pub publish`** — `ci.yml` is test-only | **interactive** — Google OAuth, expires |
| `lazily-cpp`, `lazily-zig` | GitHub Releases | tag + `gh release create` | none (persisted `gh` token) |
| `lazily-react` | npm | **not wired** — no workflows, `provenance: false` | **interactive** on first publish; npm has no pending-publisher, so the package must exist before a Trusted Publisher can be attached |
| `ctx-core` | npm ×192 | **local** `publish--force` | dead token — see blockers |

Three interactive auths in the steady state: crates.io, pub.dev, and
`lazily-react`'s first publish. Everything else is CI or a persisted token.

**Gotchas:** `lazily-kt` publishes to *immutable* GitHub Packages — check the
registry before tagging, a 409 kills the run. `lazily-zig` has no CI at all.
`coverage.json` is a hot multi-session file; commit it alone.

#### ctx-core (task #6, scope decided: changeset-affected only)

Three steps, from `ctx-core-dev/AGENTS.md`:

```sh
changeset-version-COMMIT_EDITMSG   # applies changesets, fills COMMIT_EDITMSG
recursive-git-commit               # commits per workspace package
publish--force                     # parallel bun publish; skips unchanged versions
```

`publish--force` (`lib/monorepo/bin/publish--force.sh:61`) runs
`bun publish --tag latest --access public` across **all 192 publishable packages
concurrently via GNU parallel**, comparing local version against the registry
and skipping matches. **Scope is changeset-affected only** — let changesets bump
`rmemo`, `ctx-core` and their dependents; do not force-publish the whole
monorepo.

This session changed `ctx-core/all/rmemo` (drain, error contract, `rmemo__warm`)
and `lib/rmemo` (size limits, README). Both are committed but have **no
changeset yet** — write one before versioning. Note the rmemo error-contract
change is **breaking**: a throwing memo now propagates instead of yielding
`undefined`.

#### Auth work (task #5, decided: move ctx-core into CI)

`ctx-core-dev` has **no `.github/` directory at all**. Add a tag-triggered
release workflow using one org-scoped granular npm token, so publishing leaves
the laptop and the credential is scoped and expiring rather than a long-lived
token with write access to 192 packages.

Full npm OIDC is *not* the near-term answer: Trusted Publishers are configured
**per package**, so covering `@ctx-core/*` means 192 separate registrations —
strictly more clicking than today. Stage it onto the ~14 high-blast-radius
standalone packages (`rmemo`, `relementjs`, `rebuildjs`, `esmcss`, `hyop`, …)
after the CI workflow exists.

Worth doing at the same time, each removing one interactive auth or one stored
secret:

- **crates.io** — add a release workflow to `lazily-rs` with
  `permissions: id-token: write` + `rust-lang/crates-io-auth-action@v1`, then
  register the Trusted Publisher for crate `lazily`. Highest value: also moves
  the release off the laptop.
- **pub.dev** — supports GitHub Actions publishing natively. Configure on the
  `lazily` package (repo `lazily-hub/lazily-dart`, tag pattern `v{{version}}`)
  and use `dart-lang/setup-dart`'s publish workflow. No secret at all.
- **PyPI** — replace the `PYPI_PUBLISH_TOKEN` secret in `wheels.yml` with a
  Trusted Publisher; retires a stored credential.

#### Current versions, for reference

`lazily-rs` 0.45.0 · `lazily-py` 0.33.0 · `lazily-js` 0.24.0 ·
`lazily-dart` 0.23.0 · `lazily-react` 0.1.0 (never published)

## Files worth reading first

- `src/lazily-rs/src/context.rs` — `EDGE_INDEX_THRESHOLD`, `EdgeIndex`,
  `SlotIdHasher`, `dispose_slot`, `TeardownScope`
- `src/lazily-rs/src/thread_safe.rs` — `HybridSet`/`HybridMap`,
  `EDGE_HYBRID_THRESHOLD`
- `src/lazily-spec/docs/reactive-graph.md` — the normative note
- `src/lazily-rs/examples/pubsub_load.rs` — the ladder that found most of this
