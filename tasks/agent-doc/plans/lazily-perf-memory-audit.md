# Plan: lazily-* performance + memory improvements (`#lzperfaudit`)

## Operator ask

> on lazily implementations, please identify further performance + memory
> improvements

Audit scope (operator-selected): **all ports** (rs/py/js/kt/zig/go/cpp/dart) +
**lazily-spec** + **lazily-rs as primary substrate**.

## Method

Nine parallel subagent audits (one per port + one for the spec); the substrate
(`lazily-rs`) was audited inline. Every finding cites a concrete `file:line`.
No files were modified — this is research only. The full per-port reports live
in the session transcript; this plan preserves the *actionable* subset in
priority order with stable hashtag IDs.

## What is already well-tuned (do not regress)

The reactive core of every port is mature. **Do not assume these are open
problems** — re-measure before touching:

- **lazily-rs** — per-lock-site budgets (`scripts/update-benchmark-results.py
  --check`), controlled A/B with p-values. Adopted optimizations:
  `#lzslotfastpath` (cached slot read 11.8→4.7 ns), `#lzfrontierarc`
  (BFS-time fast path), `#lzstateinvalidation` (single-lock invalidation),
  store-without-cascade, `effects_scratch` reuse, `get_rc` clone-free reads,
  `unsafe rc_clone_unchecked`, `#lzbatchborrow` (single-borrow batch flush).
- **lazily-zig** — `SlotEdgeSet` inline-edge optimization
  (`#lzedgeinline`), iterative DFS cascade, head-pointer effect flush.
- **lazily-cpp** — `SmallFn`/`SmallAny`/`SmallVec`, v0.6.0 `SmallAny`
  inline-value storage, v0.18.0 iterative frontier walk + clean-cache
  fast path.
- **lazily-dart** — 0.20.0 inlining pass (on-node value cache, iterative
  DFS cascade, small-list edges, head-pointer effect flush, lazy batch sets).
- **lazily-js** — sync core uses arrays (not Sets); `Object.freeze` for
  shape stability; isolated async path.

## Cross-cutting themes (same root cause, multiple ports)

| # | Theme | Ports | Root cause |
|---|---|---|---|
| **C1** | TextCrdt bulk-insert is O(N²) — `orderedIds()` rebuilds the origin tree per character | py, js, kt, dart, go, cpp | No maintained sibling-order index; no origin-chaining in `insertStr` |
| **C2** | `OpId` map keys are stringified (`"${counter}:${peer}"`) → per-op alloc + parse | js, dart, go | Lossy key choice |
| **C3** | `LosslessTreeCrdt.liveChildren` O(N) per parent → `Render` is O(N²) | py, kt, dart, go | No `children_by_parent` index |
| **C4** | Node edge-lists allocate eagerly even with 0 subscribers | js, kt, dart, zig (Cell subscribers), cpp (Node variant `optional<>`) | No lazy-init / SmallVec |
| **C5** | `get()` clones on every read even when caller wants a reference | rs (`get_slot` line 600), kt, dart, cpp | Only rs has `get_rc` escape hatch |
| **C6** | Two parallel deques/arrays walked in lockstep for `(id, force)` invalidation frontier | js, kt | Rust already packs as `Vec<(usize, bool)>` |
| **C7** | Wire format JSON-u8-array (4× bloat) + no string interning for repeated `type_tag`/`NodeKey` | all ports (spec-mandated) | Spec decision; needs spec relaxation |
| **C8** | Whole-state CRDT sync (CvRDT) where delta would do — only TextCrdt has `#lztextsync` | all ports (spec-mandated) | Spec needs op-based/delta variants |
| **C9** | Per-port mechanical misses: `__slots__`/`Uint8List`/`IntArray`/`SmallVec`/`@pragma('vm:prefer-inline')` | py, dart, js, cpp, zig | Local idiom drift |

## #next-steps (backlog, priority order)

### Phase 1 — mechanical CRDT fixes (highest leverage, lowest risk)

Apply C1+C2+C3 to every non-rs port. Single biggest cross-port win; unblocks
long-document editing. Add **`bench/crdt.bench.*`** in each port before/after
(none exists today — findings are inferential until measured).

- [x] **`#lztextordcache`** — Cache `orderedIds()` result + dirty flag;
      invalidate on `insert`/`delete`/`merge`/`applyDelta`. Citations:
      - `src/lazily-js/src/text-crdt.js:77` ✅ v0.21.0
      - `src/lazily-py/src/lazily/textcrdt.py:213-230` ✅ v0.32.0
      - `src/lazily-kt/src/main/kotlin/io/github/lazily/TextCrdt.kt:130-156` ✅ v0.31.0
      - `src/lazily-dart/lib/src/text_crdt.dart:123` ✅ v0.21.0
      - `src/lazily-go/text_crdt.go:162` ✅ v0.17.0
      Expected: `insertStr` O(N²)→O(N); repeated `text()` reads O(N)→O(1).
      Risk: medium — 4 mutation sites must invalidate cache.
      **Status:** landed across all 5 ports. conformance suites + Lean proofs
      green everywhere.

- [x] **`#lztextinsertchain`** — In `insertStr`, compute `visible` once at
      entry; chain subsequent chars through the just-minted OpId as
      left-origin. Spec-conformant (origin chaining is how single-peer
      type-in produces ordering naturally). Same files as `#lztextordcache`.
      Expected: 5–10× on bulk insert.
      **Status:** landed in js/kt/dart/go; **py already chained** (no-op).
      Convergence preserved (peer tiebreak on concurrent inserts).

- [x] **`#lzopidkeytuple`** — Key CRDT element maps by tuple `(counter, peer)`
      not string. Dart 3 records give free `==`/`hashCode`; Python already
      uses tuples; Rust uses `OpId` directly. Citations:
      - `src/lazily-js/src/text-crdt.js:40, 73-75, 88, 100, 107` ✅ v0.21.0 (`OpId._key` cached + `Elem.id` stored)
      - `src/lazily-dart/lib/src/text_crdt.dart:54-59, 104, 125-128, 138-139` ✅ v0.21.0 (`Map<(int, int), _TextElem>`)
      - `src/lazily-go/text_crdt.go` — **no-op**: Go already keyed `map[OpId]*textElem` directly (struct values are comparable)
      Expected: eliminates 1 string alloc + 2 parses per element per traversal.

- [x] **`#lzlivelchildidx`** — Maintain `children_by_parent` index in
      `LosslessTreeCrdt`, updated on CreateNode/Tombstone/SplitLeaf/MergeLeaves/
      Reorder. Citations:
      - `src/lazily-py/src/lazily/lossless_tree_crdt.py:720-730` ✅ v0.32.0
      - `src/lazily-kt/.../LosslessTreeCrdt.kt` (liveChildren) ✅ v0.31.0
      - `src/lazily-dart/lib/src/lossless_tree_crdt.dart` (liveChildren) ✅ v0.21.0
      - `src/lazily-go/lossless_tree_crdt.go:769` ✅ v0.17.0
      Expected: `Render` O(N²)→O(N).
      **Status:** landed across all 4 ports with LosslessTreeCrdt. Tombstones
      stay in the bucket and filter at read; tombstone/reorder mutate record
      fields in place so the index only tracks parent→child membership.

- [x] **`#lzcrdtlenfold`** — Replace `_orderedIds().length` with a fold over
      `!deleted` entries. Citations:
      - `src/lazily-dart/lib/src/text_crdt.dart:193, 202` ✅ v0.21.0
      - `src/lazily-dart/lib/src/seq_crdt.dart:344` (deferred to Phase 2; SeqCrdt.len needs separate audit)
      Expected: O(N log N)→O(N), no allocation; `tombstoneCount` benefits double.

- [x] **`#lzseqstringifyeq`** — Drop `JSON.stringify(a) === JSON.stringify(b)`
      in JS `SeqCrdt.valuesEqual`; special-case `Position` via existing
      `compareTo`, primitives via `===`, arrays via loop. Provide an optional
      user-supplied `valuesEqual` hook. Citation: `src/lazily-js/src/seq-crdt.js:96-107`.
      Expected: 5–50× on CRDT merge (V8's slowest path).
      **Status:** landed in js v0.21.0 with typed dispatch (Position/array/object).

### Phase 2 — per-port quick wins (<50 LOC, low risk)

#### lazily-py

- [x] **`#lzpylisnlogn`** ✅ v0.33.0 — Replace `reconciliation.lis_by` 2-branch recursion
      (currently O(2ⁿ), BENCHMARKS.md caps the bench at 10 keys) with
      patience-sorting O(n log n) LIS. ~15 LOC. Verify against
      `lazily-spec/conformance/collections/keyed_reconciliation_lis.json`.
      Citation: `src/lazily-py/src/lazily/reconciliation.py:96-115`.

- [x] **`#lzpyfindindex`** ✅ v0.33.0 — Add `_by_node_key: dict[(NodeId, path|None), int]`
      secondary index in `crdt_plane.CrdtPlaneRuntime`; `_find` becomes a dict
      lookup. `apply()`/`apply_frame()`/`delta_sync()` go O(N·M)→O(N+M).
      ~10 LOC. Citation: `src/lazily-py/src/lazily/crdt_plane.py:257-261`.

- [x] **`#lzpysemtreeparents`** ✅ v0.33.0 — Maintain `_parents: dict[Id, list[Node]]`
      index in SemTree. `_mark_dirty_chain` drops O(depth × N)→O(depth).
      ~20 LOC. Citation: `src/lazily-py/src/lazily/semtree.py:116-124`.

- [x] **`#lzpyreconcileidx`** ✅ v0.33.0 — In `reconcile_ops`, compute `p` once via
      `enumerate` (currently `idx_in` is O(N) linear scan inside an N-loop =
      O(N²)). 1-line fix. Citation: `src/lazily-py/src/lazily/reconciliation.py:166-213`.

- [x] **`#lzpySlotsSweep`** ✅ v0.33.0 (47 dataclasses) — Add `slots=True` to ~30 leaf `@dataclass`
      declarations (`ipc.py`, `lossless_tree_crdt.py:441-462`, `textcrdt.py:84`,
      `command.py`, `temporal.py`, `service.py`, `rateshape.py`). Mechanical.
      Expected: ~100–300 B/instance saved; multi-hundred-MB at 10M scale.

- [x] **`#lzpymatchdispatch`** ✅ v0.33.0 — Replace 6-way `isinstance` chains in
      `lossless_tree_crdt._kind_to_dict`/`_apply_op`/`_dependencies_ready` with
      `match` statements (Python 3.10+; target is 3.12+). ~50 LOC total.
      Expected: 1.5–3× dispatch speedup.

#### lazily-js

- [x] **`#lzjsscratchfrontier`** ✅ v0.23.0 — Hoist `effects`/`stack`/`forceStack` in
      `#markFrontier` to per-Context instance fields, `.length = 0` at start.
      ~15 LOC. Citation: `src/lazily-js/src/reactive.js:533-562`.
      Expected: ~10% on `set_cell_invalidation/512`.

- [x] **`#lzjsshalloweq`** ✅ v0.23.0 — Inline fast path in `defaultEqual`: `Array.isArray`
      + length check before `Object.keys`; index loop, no closure. Apply to
      4 duplicate sites (`reactive.js:17`, `reactive-async.js:19`,
      `collections.js:11`, `presence.js:68`). ~20 LOC.
      Expected: 15–30% on `memo_equality_suppression`.

- [x] **`#lzjslazyedges`** ✅ v0.23.0 — Lazy-init `dependencies`/`dependents` arrays to
      `null`; allocate on first edge. ~20 LOC across `SlotNode`/`CellNode`/
      `EffectNode`. Citation: `src/lazily-js/src/reactive.js:97-125`.
      Expected: 30–50% RSS reduction at scale (~550 B/node → ~300 B).

- [x] **`#lzjsasyncarrays`** ✅ v0.23.0 — Replace eager `new Set()` per async node with
      empty arrays; `new Map()` for `#nodes` with `[]` + `#freeIds` recycling
      (mirror the sync core). Citation: `src/lazily-js/src/reactive-async.js:105-140`.
      Expected: −40% per async node.

- [~] **`#lzjsindexsplit`** ✅ v0.23.0 (bonus: tree-shaking #lzjstreeshake + closure conv #lzjsclosure superseded the split) — Split 2,610-LOC `src/index.js` into
      `wire.js`/`snapshot.js`/`receipt.js`/`session.js`/`command.js`;
      `index.js` re-exports. Stop `ffi.js`/`shm-backend.js`/`transport.js`/
      `distributed.js` from importing through `index.js`.
      Expected: ~50% browser bundle drop for reactive-only consumers.

#### lazily-kt

- [x] **`#lzktbytearray`** ✅ v0.32.0 — Replace `IpcValue.Inline.bytes`/`NodeState.Payload.bytes`
      `List<Int>` (boxed Integer per byte) with `ByteArray`; drop eager
      `init { bytesToJson(bytes) }` validation. Citations:
      `src/lazily-kt/src/main/kotlin/io/github/lazily/Ipc.kt:170-182, 217-228`.
      Expected: ~20× memory reduction for IPC payloads.

- [x] **`#lzktsublistclear`** ✅ v0.32.0 — Replace
      `repeat(acked) { pages.removeAt(0) }` with `pages.subList(0, acked).clear()`
      (or back `pages` with `ArrayDeque`). Citation:
      `src/lazily-kt/src/main/kotlin/io/github/lazily/Relay.kt:266-271`.
      Expected: O(N²)→O(N).

- [x] **`#lzktclocklock`** ✅ v0.32.0 — Drop `@Synchronized` from `CrdtClock.tick`/
      `observe` (single-threaded reactive Context) or replace with
      `ReentrantLock`; consider `System.nanoTime()` for monotonicity. Citation:
      `src/lazily-kt/src/main/kotlin/io/github/lazily/Crdt.kt:301-323`.
      Expected: 2–3× faster HLC ops.

- [x] **`#lzktdepslist`** ✅ v0.32.0 — Drop `.toList()` in `recomputeSlotNow`'s
      `for (dep in node.dependencies.toList())`; iterate indices directly
      after moving `dependencies.clear()` to before tracking-stack push.
      Citations: `Context.kt:361`, `ThreadSafeContext.kt:385`.
      Expected: −1 ArrayList + N boxed Ints per dirty-slot recompute.

- [x] **`#lzktindexedloop`** ✅ v0.32.0 — Replace `for (b in payload)` with indexed loop
      in `ShmBlobArena.checksum`/`Ipc.bytesToJson` (Kotlin compiles `for-in`
      on primitive arrays to a boxed Iterator). Citation:
      `ShmBlobArena.kt:146-152`, `Ipc.kt:42-46`.
      Expected: 10–30% FNV checksum.

- [x] **`#lzktreliableset`** ✅ v0.32.0 — `sortedSetOf<String>()` in `ReliableSync` →
      `HashSet<String>()` (only `present()` queries it). Citation:
      `ReliableSync.kt:117-118`. Expected: O(log n)→O(1).

#### lazily-zig

- [x] **`#lzzigslotconstptr`** ✅ v0.26.0 — Change `Slot.get(self: Slot, ...)` and
      `Slot.getPtr(self: Slot, ...)` to `self: *const Slot`. All call sites
      already hold `*Slot`. Citations: `src/lazily-zig/src/lazily/context.zig:734, 755`.
      Expected: eliminates 256-byte memcpy on cached-read path.

- [x] **`#lzzigfreestack`** ✅ v0.26.0 — Add 16-entry inline free-stack to `SlotArena`;
      spill to `ArrayList` only after fill. Citation:
      `src/lazily-zig/src/lazily/context.zig:89-91`.
      Expected: removes per-destroy allocator call.

- [x] **`#lzzigcrdtstack`** ✅ v0.26.0 — `crdt.sameValues`: use a stack buffer
      `var buf: [128]bool = undefined` for `b.len ≤ 128`; passed-in allocator
      fallback otherwise. Drop the hardcoded `std.heap.page_allocator`.
      Citation: `src/lazily-zig/src/lazily/crdt.zig:196-197`.
      Expected: removes a global-lock allocation per merge.

- [x] **`#lzzigcontainsfast`** ✅ v0.26.0 — Add `SlotEdgeSet.contains(key)` fast path
      before `getOrPut` in cached-read subscribe sites. Citation:
      `src/lazily-zig/src/lazily/slot.zig:72-76`.
      Expected: halves work on cached reads with active tracking frame.

#### lazily-cpp

- [x] **`#lzcppsmallfncleanup`** ✅ v0.20.0 — `using CleanupFn = SmallFn<void(), 32>;`
      in `EffectNode`; drop the `optional<>` wrapper (SmallFn's null state is
      `vtable_ == nullptr`). Dogfoods the library's own primitive. Citations:
      `include/lazily/context.hpp:29, 166`.
      Expected: removes per-effect heap alloc for capturing cleanups.

- [x] **`#lzcppstrview`** ✅ v0.20.0 — Add `MsgUnpacker::read_str_view()`; change ~15
      decode sites to `std::string_view` keys (the unpacker buffer outlives
      decode). Citation: `include/lazily/codec.hpp:45, 66, 110, 137, 180, 189,
      205, 276, 292, 349, 356, 371, 396, 410, 446`.
      Expected: 1.5–2× decode speedup at 619 KB.

- [x] **`#lzcppunorderedmap`** ✅ v0.20.0 — `LosslessTreeCrdt`: `std::map<OpId, …>` →
      `std::unordered_map<OpId, …>` (mirrors the TextCrdt v0.6.x migration
      that yielded ~2–2.5× on `apply_delta`/`version_vector`/`delta_since`).
      Citation: `include/lazily/lossless_tree_crdt.hpp:372-373`.

- [x] **`#lzcppbarefnptr`** ✅ v0.20.0 — `std::optional<EqualsFn>` → bare
      `EqualsFn equals = nullptr;` (function pointers have a natural sentinel).
      Citation: `include/lazily/context.hpp:132`. Expected: −8 B/SlotNode
      → −80 MB RSS @ 10M cells.

- [x] **`#lzcppreservehint`** ✅ v0.20.0 — Add `MsgPacker::reserve_hint(n)` invoked once
      at top of `pack_snapshot`/`pack_delta`/`pack_crdt_sync`. Reduces
      per-`push_back` capacity checks. Citation: `include/lazily/msgpack.hpp:125-137`.

#### lazily-go

- [x] **`#lzgono reflect`** ✅ v0.18.0 — Replace `reflect.DeepEqual`/`TypeOf` at the
      three IPC/async/presence hot paths with concrete comparators.
      Citations: `src/lazily-go/ipc.go:1500`, `async_context.go:794`,
      `presence.go:192`. Expected: −50–100 ns + several allocs per call.

- [~] **`#lzgosecondary-index`** ✅ v0.18.0 (audited; no clear wins beyond Phase 1 children index) — (TBD via focused re-audit; the lazily-go
      subagent returned summary-only.) Suspected: same C1/C3 patterns as
      other ports. File follow-up audit before action.

#### lazily-dart

- [x] **`#lzdartinlines`** ✅ v0.22.0 — Add `@pragma('vm:prefer-inline')` to 10 hot
      helpers (no occurrences today). Candidates: comparators in
      `seq_crdt.dart:122`, `text_crdt.dart:28`, `lossless_tree_crdt.dart:39,179`,
      `utf8_offsets.dart:22`, `shm_blob_arena.dart:155`, `ipc.dart:1662`,
      plus core `core.dart:351, 272, 244, 255`.
      Expected: 5–15% micro; more in AOT.

- [x] **`#lzdartuint8list`** ✅ v0.22.0 — `Position.frac`/`SortKey.frac`: `List<int>` →
      `Uint8List`. Update `keyBetween` to return `Uint8List` and write into a
      `BytesBuilder` or pre-sized `Uint8List`. Citations:
      `lib/src/seq_crdt.dart:118, 139`, `lib/src/lossless_tree_crdt.dart:176, 204, 209`.
      Expected: ~1.3–2× on SeqCrdt ordering.

- [x] **`#lzdarthashint`** ✅ v0.22.0 — Replace `BigInt`-per-byte `contentHash` with the
      int math already used by the sibling `_fnv1a` in `shm_blob_arena.dart`.
      Also reconciles a latent output-range inconsistency between the two.
      Citation: `lib/src/stable_id.dart:82-94` vs `shm_blob_arena.dart:155-169`.
      Expected: 10–50× on hash; correctness fix.

- [x] **`#lzdartutf8fix`** ✅ v0.22.0 — Fix `_utf8Bytes` correctness (uses UTF-16
      `codeUnits` but branches as code points — supplementary-plane chars never
      emit 4-byte UTF-8). Switch to `s.runes` + `Uint8List` builder.
      Citation: `lib/src/stable_id.dart:96`. Bug + perf.

- [x] **`#lzdartreconcileidx`** ✅ v0.22.0 — Pre-build `commonIdxByKey` map in
      `reconcileDiff`; replace `commonKeys.indexOf(k)` with map lookup.
      Citation: `lib/src/collections.dart:584`. Expected: O(N²)→O(N log N).

- [x] **`#lzdartobservercow`** ✅ v0.22.0 — `Cell._notifyObservers`: copy-on-write list
      (replace `_observers` on subscribe/unsubscribe only) or indexed loop with
      reentrancy guard. Drop the `.toList()` per write.
      Citation: `lib/src/core.dart:474-480`.

#### lazily-rs (substrate)

- [x] **`#lzrsdeppool`** ✅ lazily-rs `3b6d7d5` — Pool the per-spawn
      `Arc::new(Mutex::new(HashSet::new()))` deps tracker in
      `spawn_async_compute` (currently 2 heap allocations per async compute
      spawn). Citations: `src/lazily-rs/src/async_context.rs:539, 1254`.
      **Status:** free-list on `AsyncContextInner` (cap 32) reusing the `Arc`
      *and* the set's table capacity; extraction switched from `.clone()` to
      `mem::take`, removing a `HashSet` clone per run. Thread-local was rejected
      — the tracker is moved into a `tokio::spawn`ed task that can complete on
      any worker. Recycling gates on `Arc::get_mut` (a leaked
      `AsyncComputeContext` keeps its tracker out of the pool) which also avoids
      the deps→inner lock-order inversion `get_cell` would deadlock against.
      **No measurable throughput change**: `async_invalidation_throughput` is
      dominated by tokio scheduling and the bench host's run-to-run drift
      (±10–27% on unmodified code) swamps it. The win is allocation-side, proven
      by `dependency_trackers_are_pooled_and_reused` rather than by a benchmark.

- [ ] **`#lzrsgetarc`** — Expose a `Send + Sync` analog to `get_rc` so
      `ThreadSafeContext` callers can read without cloning. Today
      `get_slot` always clones (line 600). Risk: medium — needs API surface
      decision.

- [ ] **`#lzrsasyncsmallany`** — `AsyncContext.set_cell` always wraps the
      value in `Arc::new(value)` (line 681). For small `Copy + Send + Sync`
      primitives, a `SmallAny`-style inline storage avoids the per-write
      allocation. Risk: medium — touches the async value contract.

### Phase 3 — algorithmic wins (spec-coordinated, medium scope)

These require **lazily-spec** changes before ports can adopt. Spec ratified in
`lazily-spec` commit `3c28120` (all seven achievable items). lazily-rs
v0.42.0 implements the three wire-format items (`#lzspecfrontiersuppress`,
`#lzspecbase64`, `#lzspecintern`) with benchmarks; the remaining items are
spec-only ratifications for ports to adopt.

- [x] **`#lzspecrevisionengine`** ✅ lazily-formal `b56aaf2` + lazily-rs v0.43.0 —
      The `get_equiv_push` Lean formal pin shipped
      (`lazily-formal/LazilyFormal/RevisionEngine.lean`). lazily-rs v0.43.0
      implements `Context::with_revision_engine()` (global R + verified_at +
      O(1) writes). Accepted by the full existing conformance suite. Crossover
      benchmarks: 1.08–1.5× write win on high-fan-out workloads. Spec ratified
      (`version-based-invalidation-plan.md` status → ratified).

- [x] **`#lzspecgcdefer`** ✅ spec `3c28120` — Ratified: GC MAY defer to idle /
      batch / incremental. Spec-only (`protocol.md` + `cell-model.md`); no wire
      change, no impl gate.

- [x] **`#lzspecobserverclarify`** ✅ spec `3c28120` — Clarified: "synchronously
      within" = glitch-free ordering, not literal in-lock dispatch
      (`reactive-graph.md` + `protocol.md`). Unblocks threaded ports from
      deferring observer dispatch.

- [x] **`#lzspecdemanddriven`** ✅ spec `3c28120` — Elevated MAY → SHOULD for
      demand-driven reader-kinds (`cell-model.md`). The 32× per-op win was
      already measured and implemented in lazily-rs; spec now matches.

- [x] **`#lzspecfrontiersuppress`** ✅ spec `3c28120` + lazily-rs v0.42.0 —
      Optional `CrdtSync.frontier` (omitted/empty = unchanged). Schema
      `distributed.json` frontier → optional; conformance fixture
      `crdt_sync_frontier_suppressed`. lazily-rs: `skip_serializing_if` +
      `CrdtSync::ops_only`. Bench: **−42% wire, −37% encode, −35% decode**.

- [x] **`#lzspecbase64`** ✅ spec `3c28120` + lazily-rs v0.42.0 — `json-base64`
      capability flag: `Inline`/`Payload` bytes as base64 strings. lazily-rs:
      `encode_json_base64`/`decode_json_base64` + `base64` optional dep.
      Bench: **42–67% wire reduction, ~27% faster decode**.

- [x] **`#lzspecintern`** ✅ spec `3c28120` + lazily-rs v0.42.0 — Sidecar batch
      string-intern table for `type_tag`. lazily-rs:
      `encode_json_intern`/`decode_json_intern`. Bench: **5% wire savings** at
      256 nodes / 4 tags (grows with node-to-tag ratio).

- [x] **`#lzspecdeltacrdt`** ✅ spec `3c28120` + lazily-rs v0.44.0 —
      `DeltaSinceRequest` control frame (FFI kind 6) + `CrdtPlaneRuntime::delta_reply`
      ships only cell states past the requester's version vector. Join is the
      same semilattice (`apply_delta` ≡ `merge`). Tests + FFI integration green.

### Phase 4 — strategic refactors (gate on real workload)

Shipped across all 6 ports. Each repo committed, pushed, tagged. Measurable wins
in wire size, per-node memory, and compute latency.

- [~] **`#lzcppsoa`** ✅ lazily-cpp v0.21.0 (partial) — Dropped `std::optional<Node>`
      for an in-variant `EmptyNode` state. Saves ~8 B/node (sizeof 104→96, ~8% RSS).
      Full per-kind SoA split (separate `slots_`/`cells_`/`effects_` vectors) deferred
      — touches ~20 `std::get_if`/`std::holds_alternative` sites.

- [~] **`#lzgoslotmap`** ✅ lazily-go v0.19.0 (investigation + benchmarks) —
      Added `BenchmarkSlotmapChaseViewport`/`Edit` as an always-run regression
      baseline. Full dense-arena refactor deferred (too risky: public types embed
      `reactiveBase` directly). Smaller-scope attempts measured and documented in
      BENCHMARKS.md; all were net-neutral or worse.

- [x] **`#lzktsmalledgelist`** ✅ lazily-kt v0.33.0 — `SmallEdgeList` (inline 0–2
      ints, promote to `ArrayList` on 3rd add) replaces all 4 edge fields
      (`Cell.dependents`, `Slot.dependencies`/`dependents`, `Effect.dependencies`).
      Dedup built in. Mirrors lazily-rs `SmallVec<[_; 2]>`.

- [x] **`#lzktasynccontextmodernize`** ✅ lazily-kt v0.33.0 — `HashMap<Int, Node>`
      → flat `Array<Node?>` arena (grow-on-overflow + LIFO recycle);
      `synchronized(Any())` → `ReentrantLock` via `locked { }` (19 sites ported).
      Matches `Context`/`ThreadSafeContext` design.

- [x] **`#lzcpppositionalcodec`** ✅ lazily-cpp v0.21.0 — Schema-versioned
      positional msgpack arrays (`pack_*_positional` / `unpack_*_positional`) for
      every wire type. **Measured 61.7% wire-size reduction** (8613 B → 3300 B on
      a 100-node Snapshot). Format-tolerant `decode()` routes MAP→string-keyed /
      ARRAY→positionally transparently.

- [x] **`#lzcppasyncmodernize`** ✅ lazily-cpp v0.21.0 — `std::function` → `SmallFn`,
      `std::shared_ptr` → `RcPtr<RcBox<…>>`. AsyncContext now dogfoods the
      library's own primitives.

- [x] **`#lzzigslotpack`** ✅ lazily-zig v0.27.0 — Slot field reorder +
      SlotEdgeSet spill `?ArrayList` → `ArrayList` (uses `items.len > 0` as
      sentinel). Slot 272 B → **256 B**; SlotEdgeSet 56 B → **48 B**.
      `#lzinplace` page-stable contract preserved.

- [x] **`#lzzigcellslotedgeset`** ✅ lazily-zig v0.27.0 — Generalized
      `SlotEdgeSet` → `EdgeSet(K, cap)`; Cell uses `EdgeSet(usize, 1)` keyed by
      callback address. Cell neutral (112 B) but **−1 heap alloc** per
      0–1-subscriber cell (the common case).

- [x] **`#lzdartstreamingjson`** ✅ lazily-dart v0.23.0 (published to pub.dev) —
      `void writeJson(StringSink sink)` on all 5 batched IPC types (Snapshot, Delta,
      CrdtSync, CrdtOp, IpcMessage). Output byte-identical to `jsonEncode(toWire())`.
      Eliminates intermediate `Map<String, Object>` allocations on the batch path.

- [x] **`#lzjsarenanodes`** ✅ lazily-js v0.24.0 — `Uint8Array` arenas for node-kind
      discriminator + packed boolean flags, moving scalar fields out of per-node
      objects. **−11% V8 heap per node** (178→159 B), **−38% cold_full_recalc**
      (265→165 ms). Closure design preserved (no class conversion).

## Recommended execution order

1. **Week 1 — Phase 1 cross-port CRDT fixes.** Land `#lztextordcache` +
   `#lztextinsertchain` first (highest cross-port leverage); add CRDT benches
   alongside. Apply `#lzopidkeytuple`, `#lzcrdtlenfold`, `#lzseqstringifyeq`
   in the same pass.
2. **Week 1 — Phase 2 per-port quick wins.** Each port ships its QW list
   independently. Land behind existing benchmark result-check tooling.
3. **Week 2 — Phase 2 continued + open Phase 3 spec issues.** Open
   lazily-spec issues for `#lzspecrevisionengine`, `#lzspecdemanddriven`,
   `#lzspecbase64`, `#lzspecdeltacrdt` so the algorithmic-tier changes can
   proceed in parallel with port work.
4. **Week 3+ — Phase 3 adoption** as conformance fixtures land. Phase 4
   refactors gated on real workload evidence (do not start speculatively).

## Notes & caveats

- **lazily-rs `make check` is red on HEAD, independent of this plan** (found
  2026-07-18 while landing `#lzrsdeppool`). Two separate pre-existing breaks:
  (a) `--all-features` failed to compile — the `#lzspecdeltacrdt` variant missed
  two feature-gated `IpcMessage` matches, which is what CI run 29627260527 was
  failing on; fixed in `3b6d7d5`. (b) `BENCHMARKS.md` is still stamped
  `version 0.40.1` against a `0.44.0` package, so
  `readme_benchmark_results_track_package_version` (`#lzbenchver`) fails — the
  v0.41–v0.44 releases never ran `make benchmark-update`. **Still open.** The
  per-item wire/latency numbers claimed in Phases 3–4 above are therefore not
  reflected in `BENCHMARKS.md`. Regenerate on a quiet host: the async benches
  drift ±10–27% run-to-run on a busy one, which would bake noise into the doc.

- **lazily-go audit is summary-only** — the dispatched agent returned the
  top finding (`reflect.DeepEqual` at three sites) but not the full
  tier-1/tier-2 detail. A focused re-audit should precede any larger go
  work. Tracked as `#lzgono reflect` + `#lzgosecondary-index`.
- **lazily-formal was out of scope.** The `get_equiv_push` Lean pin is the
  gate for `#lzspecrevisionengine` ratification — should be an explicit
  prerequisite task.
- **No CRDT benchmarks exist** in any non-rs port. The Phase 1 wins are
  inferential from algorithmic analysis; add benches before claiming
  regression budgets.
- **Conformance risk** is rated per item. Items marked "low" change only
  perf; "medium" items need paired conformance fixtures; "high" items are
  strategic and gated on measurement.
- **All findings cite exact `file:line`** in this plan; verify the line
  numbers haven't drifted before starting work.
- **Private-name hygiene**: the lazily-* submodules are public repos.
  Follow `runbooks/private-name-hygiene.md` before any edit; do not include
  agent-loop-private context (failure taxonomy, war stories, client names)
  in commit messages.
