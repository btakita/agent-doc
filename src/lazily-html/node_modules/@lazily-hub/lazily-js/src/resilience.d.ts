// Fault-tolerance primitives (#lzresilience) — the JS port.
// See resilience.js and `lazily-spec/docs/resilience.md`.

import type { Context, CellHandle } from "./reactive.js";

export type BreakerStateLabel = "Closed" | "Open" | "HalfOpen";
export const BreakerState: Readonly<{ Closed: "Closed"; Open: "Open"; HalfOpen: "HalfOpen" }>;

export class CircuitBreakerCore {
  constructor(window: number, failureThreshold: number, resetTimeout: number);
  state: BreakerStateLabel;
  allow(now: number): boolean;
  record(success: boolean, now: number): void;
}

export class CircuitBreakerCell {
  constructor(ctx: Context, window: number, failureThreshold: number, resetTimeout: number);
  readonly stateCell: CellHandle<BreakerStateLabel>;
  allow(now: number): boolean;
  record(success: boolean, now: number): void;
  state(): BreakerStateLabel;
}

export class RetryPolicyCore {
  constructor(base: number, cap: number);
  delay(attempt: number): number;
  nextDelay(): number;
  reset(): void;
}

export class RetryPolicyCell {
  constructor(ctx: Context, base: number, cap: number);
  readonly delayCell: CellHandle<number>;
  nextDelay(): number;
  reset(): void;
  delay(): number;
}

export class BulkheadCore {
  constructor(capacity: number);
  inUse: number;
  acquire(): boolean;
  release(): void;
}

export class BulkheadCell {
  constructor(ctx: Context, capacity: number);
  readonly inUseCell: CellHandle<number>;
  acquire(): boolean;
  release(): void;
  permitsInUse(): number;
}

export class TimeoutCore {
  constructor();
  arm(now: number, timeout: number): void;
  tick(now: number): boolean;
  isTimedOut(): boolean;
}

export class TimeoutCell {
  constructor(ctx: Context);
  readonly timedOutCell: CellHandle<boolean>;
  arm(now: number, timeout: number): void;
  tick(now: number): boolean;
  isTimedOut(): boolean;
}
