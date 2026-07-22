import type { AsyncSource, AsyncContext, AsyncComputed } from "./reactive-async.js";
import type { EntryKind } from "./reactive-family.js";

export { EntryKind } from "./reactive-family.js";

/** The entry handle an {@link AsyncReactiveMap} holds. */
export type AsyncMapHandle = AsyncSource | AsyncComputed;

/**
 * The async keyed reactive collection (`#reactivemap`, async flavor): keys map to
 * per-entry async reactive nodes (input cells resolved synchronously, or derived
 * slots resolved asynchronously). The transparency law is EVENTUAL — a pending
 * slot observes as `undefined` and resolves to the canonical value. Its two
 * specializations are {@link AsyncCellMap} and {@link AsyncSlotMap}.
 */
export class AsyncReactiveMap<K = unknown, V = unknown> {
  constructor(ctx: AsyncContext, kind?: EntryKind);

  /** Get the entry handle for `key`, minting via `factory(key)` if absent. */
  getOrInsertHandle(key: K, factory: (key: K) => V): AsyncMapHandle;
  /** Non-blocking observe: value for a cell/resolved slot, `undefined` if pending. */
  observe(key: K): V | undefined;
  /** Drive `key` to resolution; mint via `factory(key)` if absent. */
  resolve(key: K, factory?: (key: K) => V): Promise<V>;
  /** The existing entry handle for `key`, or `undefined`. Non-minting. */
  handle(key: K): AsyncMapHandle | undefined;
  /** Whether `key` is currently materialized. Non-reactive. */
  isPresent(key: K): boolean;
  /** Currently-materialized keys, in first-materialization order. */
  presentKeys(): K[];
  /** Number of currently-materialized entries. */
  presentCount(): number;
  /** This map's entry kind. */
  entryKind(): EntryKind;
}

/**
 * The async input-cell map: every entry is an always-resolved input cell. Adds
 * cell-only `set`.
 */
export class AsyncCellMap<K = unknown, V = unknown> extends AsyncReactiveMap<K, V> {
  constructor(ctx: AsyncContext);
  /** Set the value at `key`, inserting a new input cell if absent. Cell-only. */
  set(key: K, value: V): void;
}

/**
 * The async derived-slot map: entries are derived slots minted lazily on access
 * or eagerly via `materializeAll`, resolved via `ctx.getAsync`. NO `set`.
 */
export class AsyncSlotMap<K = unknown, V = unknown> extends AsyncReactiveMap<K, V> {
  constructor(ctx: AsyncContext);
  /** Eager materialization: pre-mint a derived slot for every key in `keys`. */
  materializeAll(keys: Iterable<K>, factory: (key: K) => V): void;
}
