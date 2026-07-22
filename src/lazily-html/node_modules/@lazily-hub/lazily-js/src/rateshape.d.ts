// Rate-shaping source operators (#lzrateshape) — the JS port.
// See rateshape.js for the logical-clock / emit-only-invalidation contract and
// `lazily-spec/docs/rate-shaping.md`.

import type { Context, CellHandle } from "./reactive.js";

export class DebounceCore<T = unknown> {
  constructor(quiet: number);
  input(now: number, v: T): void;
  tick(now: number): T | null;
}

export class DebounceCell<T = unknown> {
  constructor(ctx: Context, quiet: number);
  readonly outputCell: CellHandle<T | null>;
  input(now: number, v: T): void;
  tick(now: number): T | null;
  output(): T | null;
}

export type ThrottleEdgeLabel = "Leading" | "Trailing";
export const ThrottleEdge: Readonly<{ Leading: "Leading"; Trailing: "Trailing" }>;

export class ThrottleCore<T = unknown> {
  constructor(edge: ThrottleEdgeLabel, window: number);
  input(now: number, v: T): T | null;
  tick(now: number): T | null;
}

export class ThrottleCell<T = unknown> {
  constructor(ctx: Context, edge: ThrottleEdgeLabel, window: number);
  readonly outputCell: CellHandle<T | null>;
  input(now: number, v: T): T | null;
  tick(now: number): T | null;
  output(): T | null;
}

export type SampleModeSpec = { kind: "Count"; n: number } | { kind: "Time"; period: number };
export const SampleMode: Readonly<{
  count: (n: number) => SampleModeSpec;
  time: (period: number) => SampleModeSpec;
}>;

export class SampleCore<T = unknown> {
  constructor(mode: SampleModeSpec);
  input(v: T): T | null;
  tick(now: number): T | null;
}

export class SampleCell<T = unknown> {
  constructor(ctx: Context, mode: SampleModeSpec);
  readonly outputCell: CellHandle<T | null>;
  input(v: T): T | null;
  tick(now: number): T | null;
  output(): T | null;
}

export interface SampleRng {
  nextDouble(): number;
}

export class Lcg implements SampleRng {
  constructor(seed: number | bigint);
  nextDouble(): number;
}

export class ProbabilisticSampleCore {
  constructor(rate: number);
  readonly rate: number;
  decide(draw: number): boolean;
}

export class ProbabilisticSampleCell<T = unknown> {
  constructor(ctx: Context, rate: number, rng: SampleRng);
  readonly outputCell: CellHandle<T | null>;
  input(v: T): T | null;
  inputWithDraw(v: T, draw: number): T | null;
  output(): T | null;
}
