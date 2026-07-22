// Temporal source primitives (#lztime) — the JS port.
// See temporal.js for the logical-clock / edge-only-invalidation contract and
// `lazily-spec/docs/temporal-sources.md`.

import type { Context, CellHandle } from "./reactive.js";

/** Single-shot core: fires exactly once at the first tick with `now >= fireAt`. */
export class TimerCore {
  constructor(fireAt: number);
  fireAt: number;
  fired: boolean;
  tick(now: number): boolean;
  nextFire(): number | null;
}

/** Reactive single-shot timer: edge-only invalidation of `fired`/`value`. */
export class TimerCell {
  constructor(ctx: Context, fireAt: number);
  readonly firedCell: CellHandle<boolean>;
  tick(now: number): boolean;
  hasFired(): boolean;
  value(): true | null;
  nextFire(): number | null;
}

/** Periodic core: boundaries at `period, 2*period, ...`. */
export class IntervalCore {
  constructor(period: number);
  period: number;
  next: number;
  count: number;
  tick(now: number): boolean;
  nextFire(): number;
}

/** Reactive periodic interval: invalidates only when `count` changes. */
export class IntervalCell {
  constructor(ctx: Context, period: number);
  readonly countCell: CellHandle<number>;
  tick(now: number): boolean;
  count(): number;
  nextFire(): number;
}

/** Pattern-periodic core: fires iff `m mod cycle` is in `offsets`. */
export class CronCore {
  constructor(cycle: number, offsets: number[]);
  cycle: number;
  offsets: number[];
  cursor: number;
  count: number;
  tick(now: number): boolean;
  nextFire(): number | null;
}

/** Reactive cron source: same reactive contract as `IntervalCell`. */
export class CronCell {
  constructor(ctx: Context, cycle: number, offsets: number[]);
  readonly countCell: CellHandle<number>;
  tick(now: number): boolean;
  count(): number;
  nextFire(): number | null;
}

export type DeadlinedStateLabel = "Live" | "Expired";
export const DeadlinedState: Readonly<{ Live: "Live"; Expired: "Expired" }>;

/** Deadline core (bytes-eligible): a `TimerCore` over the deadline. */
export class DeadlineCore {
  constructor(deadline: number);
  readonly isExpired: boolean;
  tick(now: number): boolean;
  nextFire(): number | null;
}

/** Reactive value + deadline: flips `Live(v) -> Expired(v)` at the deadline. */
export class DeadlineCell<T = unknown> {
  constructor(ctx: Context, value: T, deadline: number);
  readonly expiredCell: CellHandle<boolean>;
  value: T;
  tick(now: number): boolean;
  state(): { state: DeadlinedStateLabel; value: T };
  isExpired(): boolean;
  nextFire(): number | null;
}
