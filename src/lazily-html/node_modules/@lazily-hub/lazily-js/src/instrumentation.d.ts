import type { Context, InstrumentationSnapshot } from "./reactive.js";

export type { InstrumentationSnapshot } from "./reactive.js";

/** The result of one {@link benchmark} run — timing only. */
export class BenchmarkResult {
  constructor(name: string, iterations: number, totalMicros: number);
  readonly name: string;
  readonly iterations: number;
  readonly totalMicros: number;
  /** Mean microseconds per iteration. */
  avgMicros(): number;
  /** Throughput in iterations per second. */
  opsPerSecond(): number;
  toString(): string;
}

/** Time `body` over `iterations` calls and return a {@link BenchmarkResult}. */
export function benchmark(name: string, body: () => void, iterations?: number): BenchmarkResult;

/** Run the standard reactive-core benchmark suite. */
export function runBenchmarkSuite(iterations?: number): BenchmarkResult[];

/** Run `body` against a fresh instrumented {@link Context}; return result + counters. */
export function withInstrumentation<R>(body: (ctx: Context) => R): {
  result: R;
  snapshot: InstrumentationSnapshot;
};
