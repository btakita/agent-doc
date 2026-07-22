// RelayCell backpressure plan (#relaycell), Phases 2–6. See
// lazily-spec/docs/relaycell.md and relaycell-backpressure-analysis.md.

import type { CellHandle, Context, SlotHandle } from "./reactive.js";
import type { MergePolicy } from "./merge.js";

// -- Phase 2: RelayCell + BackpressurePolicy ---------------------------------

export type BoundDim = "Count" | "Bytes" | "Keys" | "Age";
export const BoundDim: { readonly Count: "Count"; readonly Bytes: "Bytes"; readonly Keys: "Keys"; readonly Age: "Age" };

export type Overflow = "Block" | "DropNewest" | "DropOldest" | "Conflate" | "Spill";
export const Overflow: {
  readonly Block: "Block";
  readonly DropNewest: "DropNewest";
  readonly DropOldest: "DropOldest";
  readonly Conflate: "Conflate";
  readonly Spill: "Spill";
};

export const RelayConfigError: { readonly ConflateNotBounding: "ConflateNotBounding" };

export type IngressOutcome = "Accepted" | "Conflated" | "Dropped" | "Blocked";
export const IngressOutcome: {
  readonly Accepted: "Accepted";
  readonly Conflated: "Conflated";
  readonly Dropped: "Dropped";
  readonly Blocked: "Blocked";
};

/** Reactive backpressure limits (analysis §4.4). Every field is a cell. */
export class BackpressurePolicy {
  readonly ctx: Context;
  readonly dimension: CellHandle<BoundDim>;
  readonly highWater: CellHandle<number>;
  readonly lowWater: CellHandle<number>;
  readonly overflow: CellHandle<Overflow>;
  constructor(ctx: Context, dimension: BoundDim, highWater: number, lowWater: number, overflow: Overflow);
}

/** The algebra-typed conflating relay (Phase 2, in-proc core). */
export class RelayCell<T> {
  constructor(ctx: Context, policy: BackpressurePolicy, mergePolicy: MergePolicy<T>);
  overflowIsLegal(): boolean;
  /** Current window depth (Count). */
  depth(): number;
  /** Window is at/over high_water. */
  isFull(): boolean;
  /** Window is empty (nothing to drain). */
  isEmpty(): boolean;
  depthSlot(): SlotHandle<number>;
  isFullSlot(): SlotHandle<boolean>;
  isEmptySlot(): SlotHandle<boolean>;
  /** Ingest one op under the merge/overflow policy. */
  ingress(op: T): IngressOutcome;
  /** Drain the coalesced window (null when empty). */
  drain(): T | null;
  /** Peek the current coalesced window without draining. */
  peek(): T | null;
}

// -- Phase 3: SpillStore -----------------------------------------------------

export type SpillMode = "CompactOnWrite" | "AppendCompact";
export const SpillMode: { readonly CompactOnWrite: "CompactOnWrite"; readonly AppendCompact: "AppendCompact" };

/** A paged durable tail for a RelayCell (Phase 3, in-memory reference backend). */
export class SpillStore<T> {
  constructor(mode: SpillMode, pageSize: number, mergePolicy: MergePolicy<T>);
  spill(window: T, bytes: number): void;
  /** The manifest: [id, bytes] for every live page. */
  manifest(): [number, number][];
  pendingPages(): { id: number; summary: T; bytes: number }[];
  pageCount(): number;
  ackThrough(id: number): void;
  reclaim(): void;
  foldPages(s0: T): T;
  /** Reconstruction (spill_lossless): fold cold tail then hot head. */
  reconstruct(s0: T, hot: T | null): T;
  /** Crash replay of unacked pages (idempotent for an idempotent policy). */
  replayUnacked(downstream: T): T;
}

// -- Phase 4: Transport ------------------------------------------------------

export interface RelayTransport<T> {
  deliver(op: T): void;
  poll(): T[];
  hasPending(): boolean;
}

/** InProc — direct delivery: every buffered op in one frame. */
export class InProcTransport<T> implements RelayTransport<T> {
  deliver(op: T): void;
  poll(): T[];
  hasPending(): boolean;
}

/** Framed transport — bounded frames of at most frameSize (MTU/batch). */
export class FramedTransport<T> implements RelayTransport<T> {
  readonly frameSize: number;
  constructor(frameSize: number);
  deliver(op: T): void;
  poll(): T[];
  hasPending(): boolean;
}

// -- Phase 5: Outbox / Inbox roles -------------------------------------------

/** app → transport send side; backpressures the local producer via is_full. */
export class Outbox<T> {
  constructor(
    ctx: Context,
    highWater: number,
    mergePolicy: MergePolicy<T>,
    opts?: { dimension?: BoundDim; overflow?: Overflow },
  );
  send(op: T): IngressOutcome;
  drain(): T | null;
  isFull(): boolean;
  isFullSlot(): SlotHandle<boolean>;
  relay(): RelayCell<T>;
}

/** transport → app receive side; credit meter throttles the remote. */
export class Inbox<T> {
  constructor(
    ctx: Context,
    highWater: number,
    maxCredits: number,
    mergePolicy: MergePolicy<T>,
    opts?: { overflow?: Overflow },
  );
  ready(): boolean;
  credits(): number;
  receive(op: T): IngressOutcome;
  consume(replenish: number): T | null;
}

// -- Phase 6: extra reactive policies ----------------------------------------

/** Rate-limited egress (token bucket). */
export class RatePolicy {
  readonly capacity: number;
  readonly refillPerTick: number;
  constructor(capacity: number, refillPerTick: number);
  tokens(): number;
  tryEgress(): boolean;
  tick(): void;
}

/** Time-windowed coalescence (debounce/throttle). */
export class WindowPolicy {
  readonly windowOps: number;
  constructor(windowOps: number);
  onIngress(): boolean;
  tick(): boolean;
}

/** TTL / deadline expiry over a logical clock. */
export class ExpiryPolicy {
  readonly ttl: number;
  constructor(ttl: number);
  advance(by: number): void;
  now(): number;
  isLive(stampedAt: number): boolean;
  retainLive<T>(batch: [number, T][]): T[];
}

/** Priority egress — highest priority first, FIFO within priority. */
export class PriorityStorage<T> {
  push(priority: number, value: T): void;
  pop(): T | null;
  readonly length: number;
  isEmpty(): boolean;
}

/** Keyed sharding — N independent relays keyed by K. */
export class KeyedRelay<K, T> {
  constructor(ctx: Context, highWater: number, overflow: Overflow, mergePolicy: MergePolicy<T>);
  ingress(key: K, op: T): IngressOutcome;
  drain(key: K): T | null;
  keys(): IterableIterator<K>;
}
