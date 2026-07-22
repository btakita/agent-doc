// Instrumentation / benchmarks — the in-library, published measurement API.
//
// This is the isomorphic counterpart of lazily-py `lazily.benchmarks`, lazily-go
// `instrumentation.go`, and lazily-dart `instrumentation.dart`: a timing harness
// (`benchmark` / `runBenchmarkSuite`) exported from the package itself (not the
// dev-only `bench/` drivers), so downstream code can measure the reactive core in
// its own environment. It pairs with the opt-in reactive-core counters on
// {@link Context} (`instrumentationSnapshot`) — the JS-meaningful subset of
// lazily-rs's `InstrumentationSnapshot`.
//
// Timing uses `performance.now()` (isomorphic: browser + Node global). The
// dev-only criterion-style suite that generates BENCHMARKS.md lives in `bench/`.

import { Context } from "./reactive.js";
import { SlotMap } from "./reactive-family.js";

function nowMicros() {
  const perf = globalThis.performance;
  if (perf && typeof perf.now === "function") {
    return perf.now() * 1000; // ms -> us
  }
  // Last-resort fallback for exotic runtimes without `performance`.
  return Number(process?.hrtime?.bigint?.() ?? 0n) / 1000;
}

/**
 * The result of one {@link benchmark} run — timing only, matching the light-tier
 * bindings' `BenchmarkResult`.
 */
export class BenchmarkResult {
  /**
   * @param {string} name
   * @param {number} iterations
   * @param {number} totalMicros total wall time across all iterations
   */
  constructor(name, iterations, totalMicros) {
    this.name = name;
    this.iterations = iterations;
    this.totalMicros = totalMicros;
    Object.freeze(this);
  }

  /** Mean microseconds per iteration. @returns {number} */
  avgMicros() {
    return this.iterations > 0 ? this.totalMicros / this.iterations : 0;
  }

  /** Throughput in iterations per second. @returns {number} */
  opsPerSecond() {
    const avg = this.avgMicros();
    return avg > 0 ? 1e6 / avg : 0;
  }

  toString() {
    return `${this.name}: ${this.iterations} iters, ${this.avgMicros().toFixed(3)} us/iter, ${Math.round(this.opsPerSecond()).toLocaleString()} ops/s`;
  }
}

/**
 * Time `body` over `iterations` calls and return a {@link BenchmarkResult}. A
 * short warmup runs first (excluded from timing) so the measured region is warm.
 * @param {string} name
 * @param {() => void} body
 * @param {number} [iterations]
 * @returns {BenchmarkResult}
 */
export function benchmark(name, body, iterations = 1000) {
  if (typeof body !== "function") {
    throw new TypeError("benchmark body must be a function");
  }
  const warmup = Math.min(iterations, 64);
  for (let i = 0; i < warmup; i++) {
    body();
  }
  const start = nowMicros();
  for (let i = 0; i < iterations; i++) {
    body();
  }
  const totalMicros = nowMicros() - start;
  return new BenchmarkResult(name, iterations, totalMicros);
}

/**
 * Run the standard reactive-core benchmark suite and return one
 * {@link BenchmarkResult} per case. Mirrors the suite shape of the light-tier
 * bindings so cross-language numbers line up.
 * @param {number} [iterations]
 * @returns {BenchmarkResult[]}
 */
export function runBenchmarkSuite(iterations = 1000) {
  const results = [];

  results.push(
    benchmark(
      "cell_create",
      () => {
        const ctx = new Context();
        ctx.source(0);
      },
      iterations,
    ),
  );

  // cell get/set churn on a live cell.
  {
    const ctx = new Context();
    const c = ctx.source(0);
    let v = 0;
    results.push(
      benchmark(
        "cell_set_get",
        () => {
          v = (v + 1) | 0;
          ctx.set(c, v);
          ctx.get(c);
        },
        iterations,
      ),
    );
  }

  // derived slot recompute on source change.
  {
    const ctx = new Context();
    const c = ctx.source(1);
    const doubled = ctx.computed((cc) => cc.get(c) * 2);
    let v = 0;
    results.push(
      benchmark(
        "computed_recompute",
        () => {
          v = (v + 1) | 0;
          ctx.set(c, v);
          ctx.get(doubled);
        },
        iterations,
      ),
    );
  }

  // effect rerun on source change.
  {
    const ctx = new Context();
    const c = ctx.source(0);
    let sink = 0;
    ctx.effect((cc) => {
      sink += cc.get(c);
    });
    let v = 0;
    results.push(
      benchmark(
        "effect_rerun",
        () => {
          v = (v + 1) | 0;
          ctx.set(c, v);
        },
        iterations,
      ),
    );
    void sink;
  }

  // keyed map materialize-on-pull (lazy SlotMap).
  {
    const ctx = new Context();
    let k = 0;
    results.push(
      benchmark(
        "family_materialize",
        () => {
          const map = new SlotMap(ctx);
          map.getOrInsertWith(ctx, (k = (k + 1) & 1023), (key) => key * 2);
        },
        iterations,
      ),
    );
  }

  return results;
}

/**
 * Run `body` against a fresh instrumented {@link Context} and return both its
 * result and the reactive-core counter snapshot. Convenience pairing the counter
 * API with a measured region.
 * @template R
 * @param {(ctx: Context) => R} body
 * @returns {{ result: R, snapshot: import("./reactive.js").InstrumentationSnapshot }}
 */
export function withInstrumentation(body) {
  const ctx = new Context({ instrument: true });
  const result = body(ctx);
  return { result, snapshot: ctx.instrumentationSnapshot() };
}
