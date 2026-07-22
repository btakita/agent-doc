// Stream windowing (#lzwindow) — the JS port.
// See windowing.js and `lazily-spec/docs/windowing.md`.

import type { Context, CellHandle } from "./reactive.js";

export type Merge<T> = (a: T, b: T) => T;

export class TumblingCountCore<T = unknown> {
  constructor(n: number, merge: Merge<T>);
  push(v: T): T | null;
}
export class TumblingTimeCore<T = unknown> {
  constructor(period: number, merge: Merge<T>);
  push(now: number, v: T): void;
  tick(now: number): T | null;
}
export class SlidingCore<T = unknown> {
  constructor(size: number, slide: number, merge: Merge<T>);
  push(v: T): T | null;
}
export class SessionCore<T = unknown> {
  constructor(gap: number, merge: Merge<T>);
  push(now: number, v: T): T | null;
  flush(now: number): T | null;
}

export class TumblingCountWindow<T = unknown> {
  constructor(ctx: Context, n: number, merge: Merge<T>);
  readonly outputCell: CellHandle<T | null>;
  push(v: T): T | null;
  output(): T | null;
}
export class TumblingTimeWindow<T = unknown> {
  constructor(ctx: Context, period: number, merge: Merge<T>);
  readonly outputCell: CellHandle<T | null>;
  push(now: number, v: T): void;
  tick(now: number): T | null;
  output(): T | null;
}
export class SlidingWindow<T = unknown> {
  constructor(ctx: Context, size: number, slide: number, merge: Merge<T>);
  readonly outputCell: CellHandle<T | null>;
  push(v: T): T | null;
  output(): T | null;
}
export class SessionWindow<T = unknown> {
  constructor(ctx: Context, gap: number, merge: Merge<T>);
  readonly outputCell: CellHandle<T | null>;
  push(now: number, v: T): T | null;
  flush(now: number): T | null;
  output(): T | null;
}
