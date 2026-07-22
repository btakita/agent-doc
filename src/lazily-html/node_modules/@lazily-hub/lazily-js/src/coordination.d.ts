// Distributed coordination (#lzcoord) — the JS port.
// See coordination.js and `lazily-spec/docs/coordination.md`.

import type { Context, CellHandle } from "./reactive.js";

export class LeaseCore<P = unknown> {
  fence: number;
  isHeld(now: number): boolean;
  holder(now: number): P | null;
  acquire(peer: P, now: number, ttl: number): number | null;
  renew(peer: P, now: number, ttl: number): boolean;
  release(peer: P): void;
  tick(now: number): boolean;
}

export class LeaseCell<P = unknown> {
  constructor(ctx: Context);
  readonly holderCell: CellHandle<P | null>;
  acquire(peer: P, now: number, ttl: number): number | null;
  renew(peer: P, now: number, ttl: number): boolean;
  release(peer: P, now: number): void;
  tick(now: number): boolean;
  holder(now: number): P | null;
  isHeld(now: number): boolean;
  fence(): number;
}

export type LeaderRoleLabel = "Leader" | "Follower" | "Candidate";
export const LeaderRole: Readonly<{ Leader: "Leader"; Follower: "Follower"; Candidate: "Candidate" }>;

export class LeaderCell<P = unknown> {
  constructor(ctx: Context, me: P);
  readonly currentLeaderCell: CellHandle<P | null>;
  campaign(now: number, ttl: number): LeaderRoleLabel;
  contend(peer: P, now: number, ttl: number): LeaderRoleLabel;
  tick(now: number): LeaderRoleLabel;
  currentLeader(now: number): P | null;
  role(now: number): LeaderRoleLabel;
}

export class LockCell<P = unknown> {
  constructor(ctx: Context);
  readonly isLockedCell: CellHandle<boolean>;
  acquire(peer: P, now: number, ttl: number): number | null;
  release(peer: P, now: number): void;
  tick(now: number): boolean;
  validate(fence: number): boolean;
  isLocked(now: number): boolean;
  fence(): number;
}

export class SemaphoreCore {
  constructor(capacity: number);
  available(): number;
  acquire(): boolean;
  release(): void;
}

export class SemaphoreCell {
  constructor(ctx: Context, capacity: number);
  readonly permitsAvailableCell: CellHandle<number>;
  acquire(): boolean;
  release(): void;
  permitsAvailable(): number;
}

export class BarrierCore<P = unknown> {
  constructor(required: number);
  arrive(peer: P): boolean;
  count(): number;
  isOpen(): boolean;
}

export class BarrierCell<P = unknown> {
  constructor(ctx: Context, required: number);
  static quorum<P = unknown>(ctx: Context, total: number): BarrierCell<P>;
  readonly isOpenCell: CellHandle<boolean>;
  arrive(peer: P): boolean;
  count(): number;
  isOpen(): boolean;
}
