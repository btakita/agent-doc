// Large-graph scale benchmark for lazily-js.
//
// Replicates the lazily-rs `scale` group (`benches/scale.rs`), the lazily-go
// `scale` group, and lazily-py's `scale_bench.py` on a spreadsheet-shaped
// graph: `N` input cells plus `N` formula slots, where
// `formula[i] = input[i] + input[i - 1]` (local fan-in, like a column of
// `=A_i + A_{i-1}`). With the default `N = 1_000_000` that is
// ~2,000,000 reactive nodes. Four scenarios cover the spreadsheet lifecycle:
//
//   - build                     — construct all 2N nodes (formulas lazy, not yet computed).
//   - cold_full_recalc          — first read of every formula (forces every compute + edge-tracking).
//   - viewport_recalc           — edit one input, read only a bounded viewport (the lazy-pull win:
//                                 off-viewport formulas stay dirty and never recompute).
//   - full_recalc_invalidate_all — touch every input, then read every formula (worst-case full edit).
//
// Run as a script:
//
//   node bench/scale.bench.mjs
//   LAZILY_SCALE_N=1000000 node bench/scale.bench.mjs
//   LAZILY_SCALE_N=5000000 node bench/scale.bench.mjs   # Google Sheets 10M-cell workbook
//   LAZILY_SCALE_VIEWPORT=1000 node bench/scale.bench.mjs
//   BENCH_FORMAT=json node bench/scale.bench.mjs        # machine-readable output
//
// JS runs on a single event-loop thread; this is a single-threaded benchmark
// (the concurrency surfaces are correctness-tested, not benchmarked here).

import { performance } from "node:perf_hooks";
import { Context } from "../src/reactive.js";
import { blackBox } from "./harness.mjs";

export function scaleN() {
  const raw = Number(process.env.LAZILY_SCALE_N);
  return Number.isFinite(raw) && raw > 0 ? Math.floor(raw) : 1_000_000;
}

export function scaleViewport(n) {
  const raw = Number(process.env.LAZILY_SCALE_VIEWPORT);
  const vp = Number.isFinite(raw) && raw > 0 ? Math.floor(raw) : 1_000;
  return Math.min(vp, n);
}

// Construct N input cells + N formula slots (formulas lazy, not yet computed).
// formula[i] = input[i] + input[i - 1]; each read goes through ctx.getCell so
// the running slot is auto-tracked as a dependent (dynamic edge discovery).
export function buildScaleGraph(n) {
  const ctx = new Context();
  const inputs = new Array(n);
  for (let i = 0; i < n; i++) inputs[i] = ctx.cell(i);
  const formulas = new Array(n);
  for (let i = 0; i < n; i++) {
    const a = inputs[i];
    const b = i > 0 ? inputs[i - 1] : inputs[0];
    formulas[i] = ctx.computed(() => (ctx.getCell(a) + ctx.getCell(b)) >>> 0);
  }
  return { ctx, inputs, formulas };
}

function readAllFormulas(g) {
  const { ctx, formulas } = g;
  let acc = 0;
  for (let i = 0; i < formulas.length; i++) acc = (acc + ctx.get(formulas[i])) >>> 0;
  return acc;
}

function benchBuild(n) {
  const start = performance.now();
  const g = buildScaleGraph(n);
  const elapsed = (performance.now() - start) / 1000;
  const cells = 2 * n;
  blackBox(g.formulas.length);
  return {
    name: "build",
    n,
    seconds: elapsed,
    cells,
    perCellNs: (elapsed / cells) * 1e9,
    detail: `${cells.toLocaleString("en-US")} nodes`,
  };
}

function benchColdFullRecalc(n) {
  const g = buildScaleGraph(n);
  const start = performance.now();
  const sink = readAllFormulas(g);
  const elapsed = (performance.now() - start) / 1000;
  blackBox(sink);
  return {
    name: "cold_full_recalc",
    n,
    seconds: elapsed,
    cells: n,
    perCellNs: (elapsed / n) * 1e9,
    detail: `${n.toLocaleString("en-US")} formulas`,
  };
}

function benchViewportRecalc(n, samples = 1_000) {
  const vp = scaleViewport(n);
  const g = buildScaleGraph(n);
  readAllFormulas(g); // warm the whole sheet once
  const { ctx, inputs, formulas } = g;
  const mid = Math.floor(n / 2);
  const lo = Math.max(0, mid - Math.floor(vp / 2));
  const hi = Math.min(n, lo + vp);
  const window = formulas.slice(lo, hi);
  const midInput = inputs[mid];
  let acc = 0;
  const start = performance.now();
  for (let i = 0; i < samples; i++) {
    ctx.setCell(midInput, i + 1); // edit one input (monotonic → passes PartialEq guard)
    for (let j = 0; j < window.length; j++) acc = (acc + ctx.get(window[j])) >>> 0;
  }
  const elapsed = performance.now() - start; // ms total
  blackBox(acc); // defeat dead-code elimination of the viewport reads
  const perOpUs = (elapsed / samples) * 1000;
  return {
    name: "viewport_recalc",
    n,
    seconds: elapsed / 1000 / samples,
    cells: vp,
    perCellNs: null,
    detail: `${perOpUs.toFixed(2)} us/edit, viewport=${vp}, ${samples} edits`,
  };
}

function benchFullRecalcInvalidateAll(n) {
  const g = buildScaleGraph(n);
  readAllFormulas(g); // warm once
  const { ctx, inputs } = g;
  const start = performance.now();
  for (let j = 0; j < inputs.length; j++) ctx.setCell(inputs[j], j + 1); // touch every input
  const sink = readAllFormulas(g);
  const elapsed = (performance.now() - start) / 1000;
  blackBox(sink);
  return {
    name: "full_recalc_invalidate_all",
    n,
    seconds: elapsed,
    cells: n,
    perCellNs: (elapsed / n) * 1e9,
    detail: `${n.toLocaleString("en-US")} inputs re-set + ${n.toLocaleString("en-US")} formulas recomputed`,
  };
}

export function runScaleBenchmarks(n = scaleN()) {
  return [
    benchBuild(n),
    benchColdFullRecalc(n),
    benchViewportRecalc(n),
    benchFullRecalcInvalidateAll(n),
  ];
}

function fmtCell(perCellNs) {
  return perCellNs == null ? "—" : `${perCellNs.toFixed(1).padStart(8)} ns/cell`;
}

function main() {
  const n = scaleN();
  const results = runScaleBenchmarks(n);
  if (process.env.BENCH_FORMAT === "json") {
    console.log(JSON.stringify({ n, nodes: 2 * n, results }, null, 2));
    return;
  }
  console.log(
    `lazily-js scale benchmarks (N=${n.toLocaleString("en-US")}, ${(2 * n).toLocaleString("en-US")} reactive nodes)`,
  );
  for (const r of results) {
    const name = r.name.padEnd(28);
    const size = `N=${n.toLocaleString("en-US").padStart(13)}`;
    const ms = `${(r.seconds * 1000).toFixed(3).padStart(12)} ms`;
    console.log(`${name} ${size}  ${ms}  ${fmtCell(r.perCellNs)}  ${r.detail}`);
  }
}

main();
