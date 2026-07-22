// Async keyed reactive collection (`#reactivemap`, async flavor) — the
// {@link AsyncContext} analog of {@link ReactiveMap}.
//
// Keys `K` map to per-entry async reactive nodes: input cells
// ({@link AsyncSource}, always resolved) or derived slots
// ({@link AsyncComputed}, resolved asynchronously). Like the thread-safe map
// it keeps its present-set state under its own {@link AtomicMutex}.
//
// The eager/lazy behavior and present-set monotonicity are identical to the
// single-threaded map: eager pre-mints the keyset (`materializeAll`); lazy mints
// on access (`getOrInsertHandle`). There is NO eager/lazy mode flag. The
// transparency law is EVENTUAL: an async derived slot read is `undefined` while
// pending and resolves to the canonical value — so `observe` returns
// `V | undefined`. Input cells are always resolved. Drive a slot to resolution
// with `ctx.getAsync(map.get(key))` or {@link AsyncReactiveMap#resolve}.
//
// Its two specializations are {@link AsyncCellMap} (input cells) and
// {@link AsyncSlotMap} (derived slots).
//
// Rust reference: `lazily-rs/src/async_reactive_family.rs`.

import { EntryKind } from "./reactive-family.js";
import { AtomicMutex } from "./thread-safe.js";

export { EntryKind };

/**
 * The async keyed reactive collection (`#reactivemap`) generic over the entry
 * handle kind. Present-set state is guarded by an {@link AtomicMutex}; the
 * transparency law is EVENTUAL (a pending slot observes as `undefined`).
 *
 * Operations run against an owning `AsyncContext` (from `./reactive-async.js`).
 *
 * @template K, V
 */
export class AsyncReactiveMap {
  /** @type {import("./reactive-async.js").AsyncContext} */
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
   * @param {import("./reactive-async.js").AsyncContext} ctx owning async context
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
   * Mint the entry node for `key` on first access. A cell sets its value
   * directly (always resolved); a derived slot wraps `compute` in a ready async
   * recomputation. FIRST-WRITER-WINS so a raced key keeps a stable handle.
   * @param {K} key
   * @param {() => V} compute
   * @returns {any}
   */
  _mintWith(key, compute) {
    const warm = this._mutex.runExclusive(() => this._materialized.get(key));
    if (warm !== undefined) {
      return warm;
    }
    const handle =
      this._kind === EntryKind.Cell
        ? this._ctx.source(compute())
        : this._ctx.computedAsync(async () => compute());
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

  /**
   * Get the entry handle for `key`, minting it via `factory(key)` on first
   * access and caching it. For a slot map this is the {@link AsyncComputed} to
   * drive with `ctx.getAsync` or {@link AsyncReactiveMap#resolve}.
   * @param {K} key
   * @param {(key: K) => V} factory
   * @returns {any}
   */
  getOrInsertHandle(key, factory) {
    return this._mintWith(key, () => factory(key));
  }

  /**
   * Non-blocking observe of an existing entry: the value for a cell or resolved
   * slot, or `undefined` for a pending slot or an absent key. Non-minting.
   * @param {K} key
   * @returns {V | undefined}
   */
  observe(key) {
    const handle = this._mutex.runExclusive(() => this._materialized.get(key));
    if (handle === undefined) {
      return undefined;
    }
    return this._ctx.get(handle);
  }

  /**
   * Drive `key` to resolution and return its canonical value, minting the entry
   * via `factory(key)` first if absent. For a cell this is immediate; for a slot
   * it awaits the async recomputation.
   * @param {K} key
   * @param {(key: K) => V} factory
   * @returns {Promise<V>}
   */
  async resolve(key, factory) {
    const handle = factory === undefined ? this.handle(key) : this.getOrInsertHandle(key, factory);
    if (handle === undefined) {
      throw new Error(`resolve: key ${String(key)} is absent and no factory was given`);
    }
    return this._kind === EntryKind.Cell ? this._ctx.get(handle) : this._ctx.getAsync(handle);
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
 * An async INPUT-CELL map: every entry is an always-resolved input cell. The
 * async analog of {@link CellMap}. Adds cell-only `set`.
 *
 * @template K, V
 * @extends {AsyncReactiveMap<K, V>}
 */
export class AsyncCellMap extends AsyncReactiveMap {
  /** @param {import("./reactive-async.js").AsyncContext} ctx */
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
 * An async DERIVED-SLOT map: entries are derived slots minted lazily on access
 * or eagerly via {@link AsyncSlotMap#materializeAll}, resolved via
 * `ctx.getAsync`. The async analog of {@link SlotMap}.
 *
 * @template K, V
 * @extends {AsyncReactiveMap<K, V>}
 */
export class AsyncSlotMap extends AsyncReactiveMap {
  /** @param {import("./reactive-async.js").AsyncContext} ctx */
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
