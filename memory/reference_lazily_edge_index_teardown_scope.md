---
name: reference-lazily-edge-index-teardown-scope
description: "#lzspecedgeindex — per-binding edge-index thresholds (never copy one), the TeardownScope/disarm API, and how to measure a hybrid threshold correctly"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 5c0004fa-7f81-4b38-93d3-42eeb0b59a44
---

`#lzspecedgeindex`, completed 2026-07-19. Two things landed: O(1) dependency-edge
dedup across all 8 bindings, and the teardown-scope disposal contract.

## Thresholds differ per binding — never copy one

| binding | threshold | ceiling reached |
|---|---|---|
| rs | 32 (crossover ~40) | 100M |
| kt `e9d1aaf` | 32 (crossover **16**) | 10M |
| zig `4766a3c` | 64 | **100M** |
| dart `93da7c8` | 128 (crossover 60 AOT / 96 JIT) | 10M |
| js `fbc3223` | 160 | 1M |
| cpp `d266a19` | 32, demote 8 | **100M** |

go and py already hash-set edges. The crossover moves with the hash function
(rs: 170 SipHash → 40 splitmix64) **and with compilation mode** (dart: 60 AOT vs
96 JIT, no code change). The old "32 came from a JS measurement" folklore is
false — a real JS measurement gives 160, and at 32 the index is 2.3x *slower*
than the scan in JS.

## Measure the hybrid, not the pure crossover

A list arriving at degree T pays the full scan **plus** the one-time index build,
with no indexed insert at T to amortize against, so every candidate threshold
parks a regression on its own width. Sweep the real implementation against the
unfixed tree and take the knee. Two observed ways to get it wrong:

- always-indexed vs always-scanning charges index overhead on one-element lists
  to the wide list (cpp estimated 96 against a true 32);
- a width ladder's narrow rungs are noise (1.4x run-to-run at width 2–4).

**The assertion "build ns/sub grows < 2x from 1k → 1M" is unsound** — it fails a
*correct* implementation, because `notify` (which does no dedup at all) also
grows several-fold from cache/GC slopes. Use a **fan-out-2 control arm at equal
node count** and assert on the wide/narrow ratio.

Demotion thrash is real but **not universal**: 4x rs, 7.67x js, 21.5x kt, all at
exactly threshold+1 and invisible at neighbouring widths. zig has no demotion
path; dart clears its dependent list wholesale so the oscillate-by-one mode never
arises. Check the recompute shape before assuming.

## Disposal API

**`lazily-rs` ONLY — measured 2026-07-19.** Grepping implementation sources
(excluding tests/conformance) for `TeardownScope|disarm` across all eight
bindings returns hits in `lazily-rs` alone. The spec/Lean/fixture work for
`#lzspecedgeindex` step 2 shipped; **the binding API did not**, except in rs.
Confirmed independently by four conformance runners — `lazily-cpp`, `lazily-kt`,
`lazily-dart`, `lazily-go`, and `lazily-py` each skip the same 8 disposal
fixtures naming `begin_scope`/`end_scope`/`disarm`/`dispose` as unsupported.
If an earlier reading of this note suggested all-eight coverage, that was wrong;
"8 fixtures" is not "8 bindings".

Consequence: 8 of 9 `conformance/reactive-graph` fixtures are unrunnable outside
rs, and only `transitive_invalidation_reaches_depth.json` (needing just
`cell`/`computed`/`read`/`set_cell`) replays family-wide. Porting the teardown
API is what unblocks the rest. Note one fixture,
`scope_teardown_equals_fold_of_disposals.json`, is blocked by **shape** rather
than ops — it is `scenarios`-shaped and asserts a relation between two op
streams, so supporting the ops alone will never unblock it.

The API, as implemented in rs:

`ctx.scope() -> TeardownScope`, ended by drop, with `disarm()` to cancel the
teardown. `ChildContext`/`leak()` were renamed: "child context" implies nested
*visibility*, which the spec explicitly denies ("scoping bounds teardown, not
visibility"), and nothing is leaked — nodes revert to plain context ownership.
Ending a scope is proved observationally equal to disposing each member
(`disposeScope_eq_disposeAll`, `lazily-formal` `LazilyFormal.Reactive`). Graph
state is order-independent, but effect **cleanups** are side effects, so teardown
order is pinned to reverse creation order.

**GC does not substitute for disposal** — reverse edges are strong, so a
long-lived source retains every node that ever read it. Tracing bindings wanting
reachability-based reclamation must make back-edges weak. Only rs and js can
dispose slots/cells today; kt, cpp, zig, dart cannot.

## The pending-effect-queue quadratic — a SECOND defect class, all 8 audited

Not an edge-set problem, so the edge index does not cover it. A scan of the
pending/scheduled effect collection for an id that need not be there.

| binding | notify | teardown | measured |
|---|---|---|---|
| rs `1acd202`+`2b98ca6` | fixed | fixed (tombstones) | 222× / 286× / 76× |
| cpp `d266a19`+`ba9ba34` | fixed | fixed (position index) | 211.6× |
| kt `510a874` | clean | fixed | 10,677× |
| js `148e83e` | clean | fixed | 8.8× |
| dart `233e1d8`, zig `70cf3e5` | absent | absent | 90×–2230× margins, nothing found |

rs and cpp had it on **both** paths; kt and js on **teardown only**; none on
notify alone — a notify-only audit finds half of them. Edge *removal* was
separately absent in rs/kt/cpp/js and already fixed in zig/dart.

**A flag guard ("is it queued?") is NOT the fix** — the biting shape is mass
teardown during a flush, exactly when every disposed effect *is* queued.
Measured unchanged. Use tombstones (clear flag, discard at pop) or a position
index (O(1) swap-with-back, built lazily on first erase — eager build cost 1.5%
of notify for nothing).

## Measuring this class — five of five agents got it wrong first

Every agent's first saturated-teardown arm was broken, each differently: dart's
`batch()` deferred the cascade; zig's `emitChange` self-drained; cpp's stack-
ordered walk scheduled the disposer last; rs's flush order isn't creation order
(dependents are swap-removed); js's naive arm left `pendingHead` past the array
so effects silently stopped running. The first four gave **false negatives**
(flat column reads as clean); js's gave a **false positive** — a confident 186×
on a graph that had stopped working.

Countermeasures that worked, all now standard here:
- **Instrument the depth the path actually sees and assert on it** before
  trusting any timing (zig's `error.QueueNotSaturated`, rs's `--cfg audit_probe`
  reporting `queue max 0`). Commit the probe.
- **Run-count parity across arms** — every arm must do the same work (js).
- **Count iterations, don't time them**, when proving asymptotics: cpp got
  134,225,920 vs W²/2 = 134,217,728. Load-immune and exact.
- **A low forced-naive margin is evidence about your harness, not the code.**
  Real negatives ran 67×–2230×.

## Known follow-ups

- **`lazily-cpp`'s `make check` builds Release with `-DNDEBUG`, so every
  `assert()` in the test suite is compiled out** — it reported 29/29 while
  checking nothing, including tests cited as passing in the same day's edge-index
  commit. Perf results stand (bench assertions are separate code); unit-test
  evidence does not. **Check every binding for the same vacuous gate.**
- **zig use-after-free**: a slot queued in `pending_recompute` then explicitly
  `Slot.destroy`'d is not removed, so a later drain pops a freed pointer.
  Correctness, not perf; zig has no CI.
- **dart `Cell.subscribe` is copy-on-write** (`core.dart:604/613`) — O(W²) both
  directions for W observers on one cell. Third defect class, unmeasured.
- `crdt_sync_frames.json` drift blocks kt and js locally — see the CI-has-no-
  spec-sibling entry in `MEMORY.md`.

**Method note that held all day:** every conclusion reached by inspection was
wrong at least once; every one reached by bisection or clean-baseline measurement
held. Two fabricated/false claims were found *inside the plan document itself*.
Related: [[feedback_rtk_masks_make_check_exit]], [[reference_lazily_release_targets]].
