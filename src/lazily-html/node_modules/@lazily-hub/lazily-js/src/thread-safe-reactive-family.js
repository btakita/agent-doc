// Thread-safe keyed reactive collection (`#reactivemap`, thread-safe flavor) —
// the {@link ThreadSafeContext} analog of {@link ReactiveMap}.
//
// The `Send + Sync`-style analog of {@link ReactiveMap}: keys `K` map to
// per-entry reactive nodes (input cells / derived slots) allocated on a
// {@link ThreadSafeContext}. Where the single-threaded map keeps its present-set
// state inline, this map guards it with its own {@link AtomicMutex}, so a keyed
// map can be materialized concurrently from multiple realms.
//
// It obeys the same materialization laws as the single-threaded map:
//   - Eager pre-mints every declared node (`materializeAll`); lazy defers derived
//     (slot) nodes to first read (`getOrInsertWith`). There is NO mode flag.
//   - Observational transparency: a read returns an identical value whether the
//     entry was pre-minted or minted on access.
//   - Present-set monotonicity: the materialized set only grows (deferral,
//     never de-allocation).
//
// It adds MATERIALIZATION CONFLUENCE (proved in lazily-formal `Materialization` —
// `materialize_present_comm`, `materialize_observe_comm`): whatever order the
// mutex admits concurrent materializations in, the present set and every observed
// value are identical. `#mintWith` computes the node OUTSIDE the map lock, then
// commits under it with FIRST-WRITER-WINS so a raced key keeps one stable handle.
//
// Its two specializations are {@link ThreadSafeCellMap} (input cells) and
// {@link ThreadSafeSlotMap} (derived slots).
//
// Rust reference: `lazily-rs/src/thread_safe_reactive_family.rs`.

import { EntryKind } from "./reactive-family.js";
import { AtomicMutex } from "./thread-safe.js";

export { EntryKind };

/**
 * The thread-safe keyed reactive collection (`#reactivemap`) generic over the
 * entry handle kind. Present-set state is guarded by an {@link AtomicMutex};
 * materialization is confluent under concurrent access.
 *
 * @template K, V
 */
export class ThreadSafeReactiveMap {
  /** @type {import("./thread-safe.js").ThreadSafeContext} */
  _ctx;
  /** @type {EntryKind} */
  _kind;
  /** Present (materialized) entries: key -> handle. @type {Map<K, any>} */
  _materialized = new Map();
  /** First-materialization order of the present set. @type {K[]} */
  _order = [];
  /** @type {AtomicMutex} */
  _mutex = new AtomicMutex();

  /**
   * @param {import("./thread-safe.js").ThreadSafeContext} ctx owning thread-safe context
   * @param {EntryKind} [kind] entry handle kind; defaults to {@link EntryKind.Slot}
   */
  constructor(ctx, kind = EntryKind.Slot) {
    if (kind !== EntryKind.Cell && kind !== EntryKind.Slot) {
      throw new TypeError("kind must be EntryKind.Cell or EntryKind.Slot");
    }
    this._ctx = ctx;
    this._kind = kind;
  }

  /**
   * Mint the entry node for `key` (via `compute`) on first access. The node is
   * built OUTSIDE the map lock; the commit is FIRST-WRITER-WINS so a raced key
   * keeps a single stable handle (the freshly-built node is orphaned).
   * @param {K} key
   * @param {() => V} compute
   * @returns {any} the entry handle
   */
  _mintWith(key, compute) {
    // Fast path under the map lock; release before touching the context so a
    // slot recompute can't re-enter the map lock.
    const warm = this._mutex.runExclusive(() => this._materialized.get(key));
    if (warm !== undefined) {
      return warm;
    }
    const handle =
      this._kind === EntryKind.Cell ? this._ctx.source(compute()) : this._ctx.computed(() => compute());
    return this._mutex.runExclusive(() => {
      const existing = this._materialized.get(key);
      if (existing !== undefined) {
        return existing;
      }
      this._materialized.set(key, handle);
      this._order.push(key);
      return handle;
    });
  }

  /** Read a handle's value through the owning context. */
  _observe(handle) {
    return this._ctx.get(handle);
  }

  /**
   * Get the entry handle for `key`, minting it via `factory(key)` on first
   * access (the lazy pull) and caching it. Returns the same handle on repeat.
   * @param {K} key
   * @param {(key: K) => V} factory
   * @returns {any}
   */
  getOrInsertHandle(key, factory) {
    return this._mintWith(key, () => factory(key));
  }

  /**
   * Get the value at `key`, minting the entry via `factory(key)` first if
   * absent. For a {@link ThreadSafeSlotMap} this is the lazy materialization pull.
   * @param {K} key
   * @param {(key: K) => V} factory
   * @returns {V}
   */
  getOrInsertWith(key, factory) {
    return this._observe(this.getOrInsertHandle(key, factory));
  }

  /**
   * Observe `key`'s value if the entry is present, else `undefined`. Non-minting.
   * @param {K} key
   * @returns {V | undefined}
   */
  observe(key) {
    const handle = this._mutex.runExclusive(() => this._materialized.get(key));
    return handle === undefined ? undefined : this._observe(handle);
  }

  /**
   * Return the existing entry handle for `key`, or `undefined`. Non-minting.
   * @param {K} key
   * @returns {any}
   */
  handle(key) {
    return this._mutex.runExclusive(() => this._materialized.get(key));
  }

  /** Whether `key` is currently materialized (present). Non-reactive. */
  isPresent(key) {
    return this._mutex.runExclusive(() => this._materialized.has(key));
  }

  /** Currently-materialized keys, in first-materialization order. */
  presentKeys() {
    return this._mutex.runExclusive(() => [...this._order]);
  }

  /** Number of currently-materialized entries. */
  presentCount() {
    return this._mutex.runExclusive(() => this._order.length);
  }

  /** This map's entry kind. */
  entryKind() {
    return this._kind;
  }
}

/**
 * A thread-safe INPUT-CELL map: every entry is an always-materialized input
 * cell. The thread-safe analog of {@link CellMap}. Adds cell-only `set`.
 *
 * @template K, V
 * @extends {ThreadSafeReactiveMap<K, V>}
 */
export class ThreadSafeCellMap extends ThreadSafeReactiveMap {
  /** @param {import("./thread-safe.js").ThreadSafeContext} ctx */
  constructor(ctx) {
    super(ctx, EntryKind.Cell);
  }

  /**
   * Set the value at `key`, inserting a new input cell if absent. Cell-only.
   * @param {K} key
   * @param {V} value
   */
  set(key, value) {
    const existing = this._mutex.runExclusive(() => this._materialized.get(key));
    if (existing !== undefined) {
      this._ctx.set(existing, value);
      return;
    }
    this.getOrInsertHandle(key, () => value);
  }
}

/**
 * A thread-safe DERIVED-SLOT map: entries are derived slots minted lazily on
 * access or eagerly via {@link ThreadSafeSlotMap#materializeAll}. The
 * thread-safe analog of {@link SlotMap}.
 *
 * @template K, V
 * @extends {ThreadSafeReactiveMap<K, V>}
 */
export class ThreadSafeSlotMap extends ThreadSafeReactiveMap {
  /** @param {import("./thread-safe.js").ThreadSafeContext} ctx */
  constructor(ctx) {
    super(ctx, EntryKind.Slot);
  }

  /**
   * EAGER materialization: pre-mint a derived slot for every key in `keys`.
   * Observationally identical to minting each lazily on first read.
   * @param {Iterable<K>} keys
   * @param {(key: K) => V} factory
   */
  materializeAll(keys, factory) {
    for (const key of keys) {
      this.getOrInsertHandle(key, factory);
    }
  }
}
