# lazily-js Benchmark Results

Generated benchmark data for the
[`@lazily-hub/lazily-js`](https://www.npmjs.com/package/@lazily-hub/lazily-js)
reactive primitives library.

## Benchmark Results

The suite is a 1:1 port of the single-threaded `Context` benchmarks in
lazily-rs's `benches/context.rs` — the same group/case names and the same
widths/depths (`FAN_OUT_WIDTHS=[32,256]`, `MEMO_CHAIN_DEPTH=32`,
`BATCH_STORM_CELLS=64`, `SET_CELL_INVALIDATION_FAN_OUT=512`) so JS and Rust
numbers are directly comparable.

JS runs on a single event-loop thread, so the lazily-rs `ThreadSafeContext`
multi-worker contention benchmarks (`thread_safe_contention`,
`thread_safe_effect_contention`, `thread_safe_graph_propagation`) have no
like-for-like single-process counterpart here and are intentionally omitted.

<!-- benchmark-results:start -->

Generated for package `@lazily-hub/lazily-js` version `0.22.0`.

Environment: Node.js `26.4.0` on `linux x64`.

Refresh command:

```bash
node scripts/run-benchmarks.mjs
```

Mean wall-clock time per iteration; 95% CI half-width from the standard error.

| Group | Case | Mean | 95% CI | p75 | p99 | Samples |
|---|---|---:|---:|---:|---:|---:|
| cached_reads | context | 8.061 ns | ± 0.779 ns | 8.427 ns | 26.939 ns | 100 |
| cold_first_get | context | 423.300 ns | ± 43.318 ns | 342.500 ns | 1.373 us | 100 |
| dependency_fan_out | context / 32 | 3.771 us | ± 173.255 ns | 3.800 us | 7.261 us | 100 |
| dependency_fan_out | context / 256 | 47.978 us | ± 1.173 us | 47.323 us | 66.724 us | 100 |
| set_cell_invalidation | high_fan_out / 512 | 3.424 us | ± 84.646 ns | 3.410 us | 5.966 us | 100 |
| memo_equality_suppression | context | 2.783 us | ± 561.495 ns | 4.138 us | 13.663 us | 100 |
| effect_flushing | context | 188.019 ns | ± 8.596 ns | 191.316 ns | 376.753 ns | 100 |
| batch_storms | context / 64 | 14.257 us | ± 614.920 ns | 14.799 us | 26.218 us | 70 |
| typed_cache_reads | context_cell | 4.742 ns | ± 0.403 ns | 5.609 ns | 8.350 ns | 100 |
| typed_cache_reads | context_slot | 42.333 ns | ± 35.346 ns | 19.026 ns | 188.015 ns | 100 |
| default_equal | array | 34.602 ns | ± 2.774 ns | 30.943 ns | 99.200 ns | 100 |
| default_equal | object | 94.251 ns | ± 3.295 ns | 93.296 ns | 161.861 ns | 100 |
| node_allocation | scale / 4096 | 163.011 us | ± 8.360 us | 159.941 us | 322.151 us | 100 |
| textcrdt_insert_str | 64 | 9.882 us | ± 3.182 us | 8.358 us | 59.799 us | 100 |
| textcrdt_insert_str | 256 | 28.812 us | ± 2.264 us | 25.503 us | 65.357 us | 100 |
| textcrdt_insert_str | 1024 | 97.365 us | ± 5.110 us | 91.328 us | 166.233 us | 100 |
| textcrdt_repeated_text | 64 | 40.114 us | ± 1.467 us | 37.831 us | 64.145 us | 100 |
| textcrdt_repeated_text | 256 | 159.171 us | ± 8.707 us | 171.325 us | 335.391 us | 100 |
| textcrdt_repeated_text | 1024 | 915.084 us | ± 45.457 us | 1.025 ms | 1.667 ms | 100 |
| textcrdt_merge | 64 | 13.957 us | ± 1.733 us | 14.396 us | 22.914 us | 100 |
| textcrdt_merge | 256 | 65.644 us | ± 5.379 us | 69.226 us | 130.124 us | 100 |
| textcrdt_merge | 1024 | 297.418 us | ± 16.007 us | 319.127 us | 535.482 us | 100 |
| textcrdt_delta_sync | 64 | 21.147 us | ± 2.856 us | 21.657 us | 42.016 us | 100 |
| textcrdt_delta_sync | 256 | 64.549 us | ± 3.040 us | 64.218 us | 151.264 us | 100 |
| textcrdt_delta_sync | 1024 | 252.134 us | ± 11.758 us | 248.660 us | 496.058 us | 100 |
| seqcrdt_insert_back | 64 | 174.152 us | ± 173.997 us | 108.671 us | 227.421 us | 100 |
| seqcrdt_insert_back | 256 | 1.645 ms | ± 70.745 us | 1.836 ms | 2.722 ms | 100 |
| seqcrdt_insert_back | 1024 | 43.273 ms | ± 2.036 ms | 44.806 ms | 55.153 ms | 20 |
| seqcrdt_merge | 64 | 7.911 us | ± 1.244 us | 7.920 us | 23.291 us | 100 |
| seqcrdt_merge | 256 | 27.352 us | ± 2.299 us | 31.073 us | 66.754 us | 100 |
| seqcrdt_merge | 1024 | 117.426 us | ± 8.835 us | 127.046 us | 355.789 us | 100 |

<!-- benchmark-results:end -->

## Suite

| Group | Case | Parity with lazily-rs | What it measures |
|-------|------|-----------------------|------------------|
| `cached_reads` | `context` | `bench_cached_reads` / context | Steady-state cached `ctx.get(slot)` (no recompute). |
| `cold_first_get` | `context` | `bench_cold_first_get` / context | First (uncached) read of a freshly built slot. |
| `dependency_fan_out` | `context / {32,256}` | `bench_dependency_fan_out` / context | One root cell invalidates N dependents, then all are re-read. |
| `set_cell_invalidation` | `high_fan_out / 512` | `bench_set_cell_invalidation` / high_fan_out | Cost of `set` invalidating a 512-wide fan-out (no recompute). |
| `memo_equality_suppression` | `context` | `bench_memo_equality_suppression` / context | Memo chain that stays equal downstream (guard suppresses recompute). |
| `effect_flushing` | `context` | `bench_effect_flushing` / context | Effect re-runs on every cell change. |
| `batch_storms` | `context / 64` | `bench_batch_storms` / context | Coalesced batched writes to 64 cells (one effect flush). |
| `typed_cache_reads` | `context_slot`, `context_cell` | `bench_typed_cache_reads` / context_* | Direct `ctx.get(computed)` vs `ctx.get(source)`. |

### Running

```bash
npm run bench                 # run the suite, print a markdown table
node scripts/run-benchmarks.mjs        # run + refresh this file
node scripts/run-benchmarks.mjs --check # CI gate: exit 1 if stale
make bench                   # via the Makefile
```

### Harness

`bench/harness.mjs` is a zero-dependency criterion-style harness
(`node:perf_hooks` only). It offers two measurement modes:

- **`bench(group, case, fn)`** — auto-batched timing for *idempotent* routines
  (each sample times N calls, N auto-scaled so one sample lasts ~5 ms). Use for
  steady-state reads / repeated-equal-cost work.
- **`bench.batched(group, case, setup, routine)`** — `iter_batched` timing
  (criterion `BatchSize::SmallInput`): fresh `setup()` before every single
  measured call, setup excluded from timing. Use when the operation mutates graph
  state a second call would skip — e.g. the `==` guard on `set`, or a cold
  first read that caches on first access.

### Regression workflow

```bash
node scripts/run-benchmarks.mjs            # record "before" baseline in BENCHMARKS.md
# apply the performance patch
node scripts/run-benchmarks.mjs            # compare against the new numbers
```

## Scale (≥1M cells) — spreadsheet-shaped graph

A second suite ([`bench/scale.bench.mjs`](bench/scale.bench.mjs)) replicates the
lazily-rs [`scale`](https://github.com/lazily-hub/lazily-rs/blob/main/benches/scale.rs)
group, the lazily-go `scale` group, and lazily-py's
[`scale_bench.py`](https://github.com/lazily-hub/lazily-py/blob/main/src/lazily/scale_bench.py)
on a **spreadsheet-shaped** graph: `N` input cells + `N` formula slots where
`formula[i] = input[i] + input[i - 1]` (local fan-in, like a column of
`=A_i + A_{i-1}`). With the default `N = 1,000,000` that is **~2,000,000
reactive nodes**. Four scenarios cover the spreadsheet lifecycle:

- `build` — construct all `2N` nodes (formulas lazy, not yet computed).
- `cold_full_recalc` — first read of every formula (forces every compute + edge-tracking).
- `viewport_recalc` — edit one input, read only a 1,000-cell viewport (the
  lazy-pull win: off-viewport formulas stay dirty and never recompute).
- `full_recalc_invalidate_all` — re-set every input, then read every formula
  (worst-case full-sheet edit).

> **A "cell count" here counts two cells per row** — the graph models a column of
> formulas `=A_i + A_{i-1}`, so each row is **one input cell `A_i` plus one
> formula cell**. `N` rows ⇒ `N` inputs + `N` formulas = `2N` cells.

Timings use `performance.now()` (`node:perf_hooks`); single wall-clock run per
scenario. Lower is better. Treat the absolute numbers as indicative — the shapes
(relative costs, size-scaling behavior) are what transfer across runs and hosts.

### Reproduce

```bash
make bench-scale                                    # scale suite at N = 1,000,000
node bench/scale.bench.mjs                          # same, directly

# scale at a specific size / viewport:
LAZILY_SCALE_N=1000000 node bench/scale.bench.mjs
LAZILY_SCALE_N=5000000 node bench/scale.bench.mjs   # Google Sheets 10M-cell workbook
LAZILY_SCALE_VIEWPORT=1000 node bench/scale.bench.mjs
BENCH_FORMAT=json node bench/scale.bench.mjs        # machine-readable output
```

Large `N` needs headroom for the V8 heap — run with
`node --max-old-space-size=8192 bench/scale.bench.mjs` at 1M and `16384` at 5M.

### Hardware / environment

| | |
|---|---|
| CPU | AMD Ryzen 9 9950X3D (16 cores / 32 threads) |
| RAM | 186 GiB |
| OS | Linux 7.1.3 (CachyOS), x86-64 |
| Node.js | 26.4.0 (V8) |

### 1,000,000 rows (~2M cells / nodes)

Peak RSS ~1.0 GiB (~550 B/node).

| Benchmark | Time | Per cell | What it measures |
|-----------|-----:|---------:|------------------|
| `build` | ~0.90 s | ~450 ns | Construct all 2N nodes (each `Cell`/`Slot` allocates its own dependency structures — allocation- and GC-bound under V8). |
| `cold_full_recalc` | ~0.65 s | ~650 ns | First read of every formula — forces every compute + edge-tracking. |
| `viewport_recalc` | **~100 µs** | — | Edit one input, read only a 1,000-cell viewport. ~6,500× cheaper than a full cold recalc. |
| `full_recalc_invalidate_all` | ~1.30 s | ~1.30 µs | Re-set every input, then recompute the whole sheet (worst-case full-sheet edit). |

### 5,000,000 rows (10M cells — a full Google Sheets workbook)

Google Sheets caps a workbook at **10,000,000 cells**. Modeled as 5,000,000
input cells + 5,000,000 formula cells (`LAZILY_SCALE_N=5000000`). This is the
**largest size actually measured** — no extrapolation. Peak RSS ~5.0 GiB.

| Benchmark | Time | Per cell | What it measures |
|-----------|-----:|---------:|------------------|
| `build` | ~6.56 s | ~656 ns | Build the full 10M-node workbook (allocation/GC-bound). |
| `cold_full_recalc` | ~3.37 s | ~674 ns | Compute all 5M formulas cold. |
| `viewport_recalc` | **~106 µs** | — | Edit one input, read a 1,000-cell viewport. ~31,800× cheaper than a full cold recalc. |
| `full_recalc_invalidate_all` | ~7.38 s | ~1.48 µs | Re-set every input, recompute the whole workbook. |

So lazily-js backs a **full-capacity Google Sheets workbook** on V8: building it
is the expensive part (~6.6 s, allocation/GC-bound — one JS object graph per
node), but once built, a full cold recompute is ~3.4 s, and a one-cell edit +
bounded-viewport read stays in the **~100 µs range**. The lazy pull-based model
leaves off-viewport formulas dirty and never recomputes them — only ~2 formulas
actually recompute per edit (the two that read the edited input), regardless of
sheet size, which is exactly the property a viewport-rendered spreadsheet needs.

### Spreadsheet cell-count context

| Spreadsheet | Documented limit | Cells |
|-------------|------------------|------:|
| Google Sheets | 10,000,000 cells per workbook (18,278 columns max) | 10,000,000 |
| Microsoft Excel | 1,048,576 rows × 16,384 columns per worksheet | 17,179,869,184 |

The `LAZILY_SCALE_N=5000000` run above covers a full Google Sheets workbook. A
grid-complete Excel worksheet (17 billion cells) is unrepresentative — real
sheets populate a tiny fraction of the grid, and lazily only stores the cells you
create, so the `scale` group measures the populated-cell path that matters.

### Viewport scaling — flat

lazily-js's viewport recalc is **effectively size-independent** (~100 µs at 2M
nodes, ~106 µs at 10M nodes). The value cache is keyed by node identity, so both
the ~1,000 viewport cache-hit reads and the ~2 actual recomputes are O(1) map
operations that don't scale with total sheet size. This matches lazily-rs's and
lazily-py's flat curves. At 10M cells a one-cell edit + 1,000-cell viewport read
is ~31,800× cheaper than a full cold recalc and never touches off-viewport
formulas.

### Cross-language honesty

V8 is heavier per node than compiled Rust (lazily-rs builds the same 2M-node
graph in ~0.1 s vs ~0.9 s here) but far lighter than CPython (lazily-py's build
is ~10.6 s at 1M). The recompute paths (`cold_full_recalc` ~650 ns/formula) are
JIT-compiled and closer to the compiled bindings. These numbers are reported
honestly, not to claim parity — the transferable result is the **shape**: the
lazy-pull viewport property holds identically across all bindings (a one-cell
edit + bounded-viewport read is microseconds, independent of sheet size).
