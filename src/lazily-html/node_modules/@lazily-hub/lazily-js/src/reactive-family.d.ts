import type { CellHandle, ComputeOps, Context, SlotHandle } from "./reactive.js";

/**
 * Which kind of reactive node a {@link ReactiveMap} entry is — the handle-kind
 * axis the map abstracts over.
 */
export const EntryKind: {
  /** An input cell — always materialized on `get`. */
  readonly Cell: "cell";
  /** A derived slot — materialized eagerly (pre-mint) or lazily on first read. */
  readonly Slot: "slot";
};
export type EntryKind = (typeof EntryKind)[keyof typeof EntryKind];

/** The entry handle a {@link ReactiveMap} holds: an input cell or a derived slot. */
export type MapHandle<V> = CellHandle<V> | SlotHandle<V>;

/**
 * A keyed reactive collection generic over the entry handle kind (`#reactivemap`):
 * reactive membership + order, `getOrInsertWith` mint-on-access, `remove`, and
 * atomic `move`. Its two specializations are {@link CellMap} (input cells) and
 * {@link SlotMap} (derived slots).
 */
export class ReactiveMap<K = unknown, V = unknown> {
  constructor(ctx: Context, kind?: EntryKind);

  /**
   * Get the value at `key`, minting via `factory(key)` if absent (mint-on-access).
   * `ops` is the reactive read surface — the `Compute` view inside a compute/effect
   * closure (subscribes the caller), else the owning `Context` (untracked).
   */
  getOrInsertWith(ops: ComputeOps, key: K, factory: (key: K) => V): V;
  /** The existing entry handle for `key`, or `undefined`. Non-reactive. */
  handle(key: K): MapHandle<V> | undefined;
  /** Read the value at `key` if present, else `undefined`. Reactive on that entry. */
  get(ops: ComputeOps, key: K): V | undefined;
  /** Remove `key`'s entry; bumps membership. Returns whether it was present. */
  remove(key: K): boolean;
  /** Reactive snapshot of keys in order (subscribes to order changes via `ops`). */
  keys(ops: ComputeOps): K[];
  /** Currently-materialized keys, in first-materialization order. Non-reactive. */
  presentKeys(): K[];
  /** Number of currently-materialized entries. Non-reactive. */
  presentCount(): number;
  /** Whether `key` is currently materialized. Non-reactive. */
  isPresent(key: K): boolean;
  /** Current 0-based position of `key`, or `undefined` if absent. Non-reactive. */
  position(key: K): number | undefined;
  /** Atomically move `key` to `index` (`#lzcellmove`). Returns whether present. */
  moveTo(key: K, index: number): boolean;
  /** Atomically move `key` to just before `anchor`. */
  moveBefore(key: K, anchor: K): boolean;
  /** Atomically move `key` to just after `anchor`. */
  moveAfter(key: K, anchor: K): boolean;
  /** Reactive entry count (subscribes to membership changes only, via `ops`). */
  len(ops: ComputeOps): number;
  /** Reactive emptiness check (subscribes to membership changes, via `ops`). */
  isEmpty(ops: ComputeOps): boolean;
  /** Reactive membership test for `key` (subscribes to membership changes, via `ops`). */
  containsKey(ops: ComputeOps, key: K): boolean;
  /** Non-reactive count. */
  lenUntracked(): number;
  /** This map's entry kind. */
  entryKind(): EntryKind;
}

/**
 * The input-cell specialization of {@link ReactiveMap}: adds cell-only `set` and
 * eager value-minting (`entry` / `entryWith`).
 */
export class CellMap<K = unknown, V = unknown> extends ReactiveMap<K, V> {
  constructor(ctx: Context);
  /** Return the cell for `key`, minting with `defaultFn()` on first access. */
  entryWith(key: K, defaultFn: () => V): CellHandle<V>;
  /** Return the cell for `key`, minting with `defaultValue` on first access. */
  entry(key: K, defaultValue: V): CellHandle<V>;
  /** Set the value at `key`, inserting a new input cell if absent. Cell-only. */
  set(key: K, value: V): void;
}

/**
 * The derived-slot specialization of {@link ReactiveMap}: `getOrInsertWith` mints
 * a slot on first access (lazy); `materializeAll` pre-mints the keyset (eager).
 * NO `set`.
 */
export class SlotMap<K = unknown, V = unknown> extends ReactiveMap<K, V> {
  constructor(ctx: Context);
  /** Eager materialization: pre-mint a derived slot for every key in `keys`. */
  materializeAll(keys: Iterable<K>, factory: (key: K) => V): void;
}
