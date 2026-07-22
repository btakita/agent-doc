// Criterion-style micro-benchmark harness for lazily-js.
//
// Mirrors the lazily-rs `benches/context.rs` measurement model (criterion 0.8):
//   - auto-batched timing for *idempotent* routines (each sample times N calls,
//     N auto-scaled so one sample lasts ~TARGET_SAMPLE_MS), and
//   - iter_batched timing for *non-idempotent* routines (fresh `setup()` before
//     every single measured call, setup excluded from the timed region), so a
//     case like `setCell(root, 1)` is always a real 0->1 invalidation rather
//     than a `==`-guarded no-op on the second call.
//
// Zero dependencies (node:perf_hooks only) — keeps the published package lean.
// Emits a Group/Case/Mean/95% CI/p75/p99/Samples markdown table matching
// lazily-rs's BENCHMARKS.md so cross-language numbers are directly comparable.

import { performance } from "node:perf_hooks";

// Sink the optimizer cannot eliminate — black_box analog (force a read+write
// through a module-level mutable captured by an exported function).
let __blackHole = undefined;
export function blackBox(value) {
  __blackHole = value;
  return value;
}
export function doNotOptimize(value) {
  return blackBox(value);
}

const TARGET_SAMPLE_MS = 5; // ~criterion measurement_time per sample
const MIN_SAMPLES = 20;
const MAX_SAMPLES = 100;
const WARMUP_ITERS = 64;
const NS_PER_MS = 1e6;
const MIN_RUN_MS = 500;

function quantile(sorted, q) {
  if (sorted.length === 0) return 0;
  const pos = q * (sorted.length - 1);
  const lo = Math.floor(pos);
  const hi = Math.ceil(pos);
  if (lo === hi) return sorted[lo];
  const frac = pos - lo;
  return sorted[lo] * (1 - frac) + sorted[hi] * frac;
}

function summarize(samplesNs) {
  const sorted = [...samplesNs].sort((a, b) => a - b);
  const mean = sorted.reduce((a, v) => a + v, 0) / sorted.length;
  const variance =
    sorted.reduce((a, v) => a + (v - mean) ** 2, 0) / sorted.length;
  const sem = Math.sqrt(variance / sorted.length);
  return {
    mean,
    p50: quantile(sorted, 0.5),
    p75: quantile(sorted, 0.75),
    p99: quantile(sorted, 0.99),
    samples: sorted.length,
    ci: 1.96 * sem, // 95% CI half-width (t≈1.96 for n≥20)
  };
}

export function format(ns) {
  if (ns >= 1e6) return `${(ns / 1e6).toFixed(3)} ms`;
  if (ns >= 1e3) return `${(ns / 1e3).toFixed(3)} us`;
  return `${ns.toFixed(3)} ns`;
}

// Auto-batched measurement for an idempotent routine (criterion `iter`).
function measure(fn, { minMs = MIN_RUN_MS } = {}) {
  for (let i = 0; i < WARMUP_ITERS; i++) fn();
  // Calibrate iteration count so one sample hits TARGET_SAMPLE_MS.
  const t0 = performance.now();
  fn();
  const oneNs = (performance.now() - t0) * NS_PER_MS;
  let iters = oneNs > 0 ? Math.max(1, Math.ceil((TARGET_SAMPLE_MS * NS_PER_MS) / oneNs)) : 1;

  const samples = [];
  let elapsed = 0;
  while ((samples.length < MIN_SAMPLES || elapsed < minMs) && samples.length < MAX_SAMPLES) {
    const start = performance.now();
    for (let i = 0; i < iters; i++) fn();
    const dtMs = performance.now() - start;
    samples.push((dtMs * NS_PER_MS) / iters);
    elapsed += dtMs;
  }
  return summarize(samples);
}

// iter_batched measurement: fresh setup() before every single measured call,
// setup excluded from timing (criterion `iter_batched`, BatchSize::SmallInput).
function measureBatched(setup, routine, { minMs = MIN_RUN_MS, minSamples = MIN_SAMPLES } = {}) {
  for (let i = 0; i < WARMUP_ITERS; i++) routine(setup());
  const samples = [];
  let elapsed = 0;
  while ((samples.length < minSamples || elapsed < minMs) && samples.length < MAX_SAMPLES) {
    const ctx = setup();
    const start = performance.now();
    routine(ctx);
    const ns = (performance.now() - start) * NS_PER_MS;
    samples.push(ns);
    elapsed += ns / NS_PER_MS;
  }
  return summarize(samples);
}

const registry = [];

// Idempotent routine: the function is called many times per sample, so it must
// be safe to repeat (each call performs the same amount of real work).
export function bench(group, caseLabel, fn, opts = {}) {
  registry.push({ group, case: caseLabel, kind: "fn", fn, opts });
}

// Non-idempotent routine: setup() runs before every single measured call and is
// excluded from timing; routine(ctx) runs once per sample. Use this whenever the
// operation mutates graph state that a second call would skip (e.g. the `==`
// guard on setCell, or a cold first read that caches on first access).
bench.batched = function batched(group, caseLabel, setup, routine, opts = {}) {
  registry.push({ group, case: caseLabel, kind: "batched", setup, routine, opts });
};

export async function run({ print = true, format = "markdown" } = {}) {
  const results = [];
  for (const entry of registry) {
    let stats;
    if (entry.kind === "fn") stats = measure(entry.fn, entry.opts);
    else stats = measureBatched(entry.setup, entry.routine, entry.opts);
    results.push({ group: entry.group, case: entry.case, ...stats });
  }
  if (print) {
    if (format === "json") console.log(JSON.stringify({ results }, null, 2));
    else printMarkdown(results);
  }
  return results;
}

function printMarkdown(results) {
  console.log("| Group | Case | Mean | 95% CI | p75 | p99 | Samples |");
  console.log("|---|---|---:|---:|---:|---:|---:|");
  for (const r of results) {
    console.log(
      `| ${r.group} | ${r.case} | ${format(r.mean)} | ± ${format(r.ci)} | ${format(r.p75)} | ${format(r.p99)} | ${r.samples} |`,
    );
  }
}
