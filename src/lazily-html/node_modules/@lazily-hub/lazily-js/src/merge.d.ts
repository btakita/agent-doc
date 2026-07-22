// Phase 1 of the RelayCell backpressure plan (#relaycell) — merge algebra +
// the Reactive/Source read/write split. See lazily-spec/docs/reactive-graph.md
// § "MergeCell and the merge algebra".

import type { CellHandle, ComputeOps, Context } from "./reactive.js";

/** An associative merge policy `⊕: T×T→T` with its transport-selected flags. */
export interface MergePolicy<T> {
  readonly name: string;
  /** The associative fold. MUST satisfy `(a⊕b)⊕c == a⊕(b⊕c)`. */
  merge(old: T, op: T): T;
  /** `⊕` is commutative (reordering tax). */
  readonly commutative: boolean;
  /** `⊕` is idempotent — `(a⊕b)⊕b == a⊕b` (durability tax). */
  readonly idempotent: boolean;
  /** Conflation bounds the state (the `Conflate` overflow precondition). */
  readonly conflates: boolean;
}

export const KeepLatest: MergePolicy<unknown>;
export const Sum: MergePolicy<number>;
export const Max: MergePolicy<number>;
export const SetUnion: MergePolicy<Set<unknown>>;
export const RawFifo: MergePolicy<unknown[]>;

/** A cell whose write is a merge under `policy` (`Cell ≡ MergeCell(KeepLatest)`). */
export class MergeCell<T> {
  readonly ctx: Context;
  readonly cell: CellHandle<T>;
  readonly policy: MergePolicy<T>;
  constructor(ctx: Context, cell: CellHandle<T>, policy: MergePolicy<T>);
  /**
   * Read the current converged value. Thread the {@link ComputeOps} view (`cx`)
   * a compute/effect closure received to register a dependency edge; a bare
   * `get()` at top level reads untracked.
   */
  get(cx?: ComputeOps): T;
  /** Replace the value (keep-latest write). */
  set(value: T): void;
  /** Fold `op` into the value under the policy. */
  merge(op: T): void;
}

/** Create a `MergeCell` over `ctx`. */
export function mergeCell<T>(ctx: Context, initial: T, policy: MergePolicy<T>): MergeCell<T>;

/** Adapt a plain cell to the `Source` shape (get/set/merge; merge == replace). */
export function asSource<T>(
  ctx: Context,
  cellHandle: CellHandle<T>,
): { get(): T; set(value: T): void; merge(op: T): void };

/** Adapt any read handle to the `Reactive` shape (`{ get }`). */
export function asReactive<T>(ctx: Context, handle: unknown): { get(): T };
