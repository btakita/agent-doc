// Reactive-graph benchmarks for lazily-js — parity port of lazily-rs
// `benches/context.rs` (the single-threaded `Context` variants).
//
// JS runs on a single event-loop thread, so the lazily-rs `ThreadSafeContext`
// multi-worker contention benchmarks (thread_safe_contention,
// thread_safe_effect_contention, thread_safe_graph_propagation) have no
// like-for-like single-process counterpart here and are intentionally omitted.
// Every group/case below is a 1:1 port of the matching `context`-variant in
// lazily-rs, using the same widths/depths so numbers are cross-language
// comparable. Constants mirror `benches/context.rs` exactly.

import { bench, blackBox, run } from "./harness.mjs";
import { Context, defaultEqual } from "../src/reactive.js";

const FAN_OUT_WIDTHS = [32, 256];
const MEMO_CHAIN_DEPTH = 32;
const BATCH_STORM_CELLS = 64;
const SET_CELL_INVALIDATION_FAN_OUT = 512;

// --- setup helpers (ports of setup_context_* in benches/context.rs) ---------

function setupFanOut(width) {
  const ctx = new Context();
  const root = ctx.cell(0);
  const slots = [];
  for (let offset = 0; offset < width; offset++) {
    slots.push(ctx.computed(() => (ctx.getCell(root) + offset) >>> 0));
  }
  for (const slot of slots) blackBox(ctx.get(slot));
  return { ctx, root, slots };
}

function setupMemoChain(depth) {
  const ctx = new Context();
  const root = ctx.cell(0);
  let tail = ctx.memo(() => ctx.getCell(root) % 2);
  for (let i = 0; i < depth; i++) {
    const previous = tail;
    tail = ctx.computed(() => (ctx.get(previous) + 1) >>> 0);
  }
  blackBox(ctx.get(tail));
  return { ctx, root, tail };
}

function setupBatchStorm(cellsLen) {
  const ctx = new Context();
  const cells = [];
  for (let idx = 0; idx < cellsLen; idx++) cells.push(ctx.cell(idx));
  let sink = 0;
  const effectCells = cells.slice();
  ctx.effect(() => {
    let total = 0;
    for (const cell of effectCells) total = (total + ctx.getCell(cell)) >>> 0;
    sink = total;
  });
  return { ctx, cells, getSink: () => sink };
}

// --- cached_reads: steady-state cached slot read ----------------------------
// Parity: bench_cached_reads "context" in lazily-rs/context.rs.
// Setup runs once before registration; only the cached `get` is timed.
{
  const ctx = new Context();
  const root = ctx.cell(21);
  const doubled = ctx.computed(() => ctx.getCell(root) * 2);
  blackBox(ctx.get(doubled)); // prime the cache
  bench("cached_reads", "context", () => blackBox(ctx.get(blackBox(doubled))));
}

// --- cold_first_get: first (uncached) read of a freshly built slot ---------
// Parity: bench_cold_first_get "context" (iter_batched — fresh ctx each call).
bench.batched(
  "cold_first_get",
  "context",
  () => {
    const ctx = new Context();
    const root = ctx.cell(21);
    const doubled = ctx.computed(() => ctx.getCell(root) * 2);
    return { ctx, doubled };
  },
  ({ ctx, doubled }) => blackBox(ctx.get(blackBox(doubled))),
);

// --- dependency_fan_out: one root cell invalidates N dependents ------------
// Parity: bench_dependency_fan_out "context / {32,256}" (iter_batched).
for (const width of FAN_OUT_WIDTHS) {
  bench.batched(
    "dependency_fan_out",
    `context / ${width}`,
    () => setupFanOut(width),
    ({ ctx, root, slots }) => {
      ctx.setCell(root, blackBox(1));
      let total = 0;
      for (const slot of slots) total = (total + ctx.get(slot)) >>> 0;
      blackBox(total);
    },
  );
}

// --- set_cell_invalidation: cost of invalidating a wide fan-out ------------
// Parity: bench_set_cell_invalidation "high_fan_out / 512" (iter_batched).
// Only the setCell invalidation is timed (not the downstream recompute) —
// matches the lazily-rs routine which times `set_cell` + `black_box(slots.len())`.
bench.batched(
  "set_cell_invalidation",
  "high_fan_out / 512",
  () => setupFanOut(SET_CELL_INVALIDATION_FAN_OUT),
  ({ ctx, root, slots }) => {
    ctx.setCell(root, blackBox(1));
    blackBox(slots.length);
  },
);

// --- memo_equality_suppression: memo chain that stays equal downstream -----
// Parity: bench_memo_equality_suppression "context" (iter_batched).
// root%2 memo head; setting root to an even value keeps the head's value equal
// (0) so downstream caches survive — the memo guard path.
bench.batched(
  "memo_equality_suppression",
  "context",
  () => setupMemoChain(MEMO_CHAIN_DEPTH),
  ({ ctx, root, tail }) => {
    ctx.setCell(root, blackBox(2));
    blackBox(ctx.get(blackBox(tail)));
  },
);

// --- effect_flushing: effect re-runs on every cell change ------------------
// Parity: bench_effect_flushing "context" (idempotent: advances next each call
// so each setCell is a real 1->2->3 ... change, not a ==-guarded no-op).
{
  const ctx = new Context();
  const root = ctx.cell(0);
  let seen = 0;
  ctx.effect(() => {
    seen = (seen + ctx.getCell(root)) >>> 0;
  });
  let next = 0;
  bench("effect_flushing", "context", () => {
    next = (next + 1) >>> 0;
    ctx.setCell(root, blackBox(next));
    blackBox(seen);
  });
}

// --- batch_storms: coalesced batched writes to many cells -----------------
// Parity: bench_batch_storms "context / 64".
{
  const { ctx, cells, getSink } = setupBatchStorm(BATCH_STORM_CELLS);
  let base = BATCH_STORM_CELLS;
  bench("batch_storms", `context / ${BATCH_STORM_CELLS}`, () => {
    base = (base + BATCH_STORM_CELLS) >>> 0;
    ctx.batch(() => {
      for (let i = 0; i < cells.length; i++) {
        ctx.setCell(cells[i], blackBox((base + i) >>> 0));
      }
    });
    blackBox(getSink());
  });
}

// --- typed_cache_reads: direct slot/cell reads ----------------------------
// Parity: bench_typed_cache_reads "context_slot" / "context_cell".
{
  const ctx = new Context();
  const cell = ctx.cell(42);
  const slot = ctx.computed(() => ctx.getCell(cell));
  blackBox(ctx.get(slot)); // prime
  bench("typed_cache_reads", "context_slot", () => blackBox(ctx.get(blackBox(slot))));
}
{
  const ctx = new Context();
  const cell = ctx.cell(99);
  bench("typed_cache_reads", "context_cell", () => blackBox(ctx.getCell(blackBox(cell))));
}

// --- default_equal: structural object comparison (#lzjsshalloweq) -----------
// Times the equality guard hot path on its own (object + array shapes). The
// inline Array.isArray fast path + index loop is the Phase 2 win.
{
  const a = { id: 42, name: "alice", tags: ["x", "y", "z"], n: 7, active: true };
  const b = { id: 42, name: "alice", tags: ["x", "y", "z"], n: 7, active: true };
  bench("default_equal", "object", () => blackBox(defaultEqual(a, b)));
}
{
  const a = [1, 2, 3, 4, 5, 6, 7, 8];
  const b = [1, 2, 3, 4, 5, 6, 7, 8];
  bench("default_equal", "array", () => blackBox(defaultEqual(a, b)));
}

// --- node_allocation: node creation at scale (#lzjslazyedges) ---------------
// Allocates a mix of cells, memos, and effects (no edges) so the per-node
// constructor cost — now free of eager [] edge arrays — is what is timed.
// The lazy-edges win is primarily RSS; this case guards the constructor
// time regression and surfaces the allocation speedup.
const ALLOC_NODES = 4096;
bench.batched(
  "node_allocation",
  `scale / ${ALLOC_NODES}`,
  () => ({ ctx: new Context() }),
  ({ ctx }) => {
    for (let i = 0; i < ALLOC_NODES; i++) {
      ctx.cell(i);
    }
    blackBox(ctx);
  },
);

await run({
  format: process.env.BENCH_FORMAT === "json" ? "json" : "markdown",
});
