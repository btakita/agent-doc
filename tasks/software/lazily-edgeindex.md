# lazily #lzspecedgeindex — fan-out, disposal, and the family port

Continuation plan. Session of 2026-07-19 fixed an O(n²) dependency-edge defect
in `lazily-rs` and, chasing it with a load test, found four more. Five of eight
bindings still carry the original defect and six lack disposal entirely.

## State

Green: `make check` passes in `lazily-rs` and `lazily-spec`. Trees clean.

| repo | commits | note |
|---|---|---|
| `src/lazily-rs` | 15 | edge index, hasher, disposal, `ChildContext` |
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
cargo run --release --example child_context_spike       # task 12 option C (shipped)
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

`ChildContext` vs disposing by hand, per node: build 156.9 → 72.5 ns @1024,
teardown 61.1 → 56.1 ns @65,536.

## What was wrong, and what it cost to find

Worth reading before touching the other bindings, because every one of these is
a trap the ports will hit.

1. **The threshold is not portable.** 32 came from a JS measurement. In Rust the
   crossover measured near 170 with SipHash and near 40 once ids were hashed
   with splitmix64 — a 4× shift from changing the hash function alone. Shipping
   32 made widths 33–170 *slower than before the fix*. Now 32 again, for a
   different reason.
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
- `lazily-kt`, `lazily-cpp`, `lazily-zig`, `lazily-dart` — unconditional linear
  scan in edge dedup, same defect
- `lazily-js` — same; a hybrid prototype measured 11.4× at n=4000 with no narrow
  regression, not yet applied
- `lazily-go`, `lazily-py` — already hash-set edges, no change needed

Brief each agent with `lazily-spec/docs/reactive-graph.md` (the
`#lzspecedgeindex` implementation note), not with lazily-rs's constants. Require
a measured before/after from a clean baseline per binding. Reject any
performance claim without one — inventing plausible numbers is the likely
failure mode.

`lazily-zig` has an extra defect: `EdgeSet.remove` (`context.zig:558`) is a
linear scan called during cascade, an O(in-degree) cost per edge on wide fan-in
that no other binding has.

### 2. Family disposal decision (task #11) — needs a human

Only `lazily-js` and `lazily-rs` can dispose slots/cells. go/py/kt cannot, and
GC does not save them: `lazily-py/src/lazily/cell.py:28` holds
`_parents: set[Slot]` and `lazily-go/core.go:54` holds
`dependents map[reactiveNode]struct{}` — both strong, so a long-lived topic
keeps every slot that ever read it. cpp/zig/dart unchecked.

Three mechanisms, all spiked and measured in `lazily-rs` (see task #12):
explicit disposal, `ChildContext` groups, and `Rc`/`Weak` handles. Recommended:
groups + explicit, both on `Copy` handles. Only `Rc`/`Weak` closes the
cross-owner hazard, and it costs `Copy` — a `.clone()` at every fan-out capture
site, or a gtk-rs-style `clone!` macro. The GC'd bindings may want weak
back-edges instead, which is what `ctx-core`'s rmemo does and needs no API at
all. Decide before writing the same thing five times.

### 3. Flatten per-subscriber cost (task #10) — gated, in order

Growth past 1M is memory-bound, not algorithmic: notify jumps 60 → 151 ns/sub
exactly where the working set crosses this box's 128 MiB L3.

1. Measure where 247 B/node goes. Hypothesis, **unverified**: a node is 3–4
   scattered allocations (arena entry, `Rc<dyn Fn>` closure, `Arc<dyn Any>`
   value), so one logical touch is several cache misses.
2. Test transparent huge pages. 23.6 GiB in 4 KiB pages is ~5.9M PTEs.
3. Only then consider a dense flag arena for invalidation. Marking needs one bit
   but pays a full node touch; `lazily-js` already does this
   (`#lzjsarenanodes`). Would target the 865 ms unread publish at 100M.

Do not start 3 before 1 and 2.

### 4. Release (tasks #5, #6) — blocked

`npm whoami` returns **E401**; the stored token is dead, so `publish--force`
would fail on all 192 `ctx-core` packages. Needs a human re-login, and that is
the moment to switch to an org-scoped granular token and move publishing into
CI — `ctx-core-dev` has no `.github/` directory at all. Full audit is in the
session history; crates.io and pub.dev also need one interactive auth each.

## Files worth reading first

- `src/lazily-rs/src/context.rs` — `EDGE_INDEX_THRESHOLD`, `EdgeIndex`,
  `SlotIdHasher`, `dispose_slot`, `ChildContext`
- `src/lazily-rs/src/thread_safe.rs` — `HybridSet`/`HybridMap`,
  `EDGE_HYBRID_THRESHOLD`
- `src/lazily-spec/docs/reactive-graph.md` — the normative note
- `src/lazily-rs/examples/pubsub_load.rs` — the ladder that found most of this
