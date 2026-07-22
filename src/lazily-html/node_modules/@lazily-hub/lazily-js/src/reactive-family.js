// Keyed reactive collections: the generic `ReactiveMap` and its `CellMap` /
// `SlotMap` specializations (`#reactivemap`).
//
// `lazily-spec/cell-model.md` § "Keyed cell collections" fixes ONE keyed
// primitive, generic over the entry's handle kind (`ReactiveMap<K, V, H>`):
// reactive membership + order, `getOrInsertWith` (mint-on-access), `remove`,
// and atomic `move`. Its two specializations are the concrete types a binding
// exposes:
//
//   - `CellMap<K, V>` = `ReactiveMap` over the input-cell handle. Adds cell-only
//     `set(key, value)` (an input is settable) and eager value-minting
//     (`entry` / `entryWith`). Minting is eager-by-value.
//   - `SlotMap<K, V>` = `ReactiveMap` over the derived-slot handle.
//     `getOrInsertWith(key, factory)` mints a slot on first access (LAZY
//     materialization); a slot's value is derived, so `SlotMap` has NO `set`.
//     Eager materialization is `materializeAll` — a pre-mint loop over the
//     keyset. There is NO eager/lazy mode flag.
//
// The shared surface — `getOrInsertWith` / `remove` / `move*` / membership /
// order / `keys` / `len` / `containsKey` — lives on the generic `ReactiveMap`.
// `set` and eager value-minting are the `CellMap`-only specialization; the
// pre-mint eager helper is the `SlotMap`-only specialization. There are NO
// family types: the "keyed materialized family" is `SlotMap` + the mint recipe,
// and the "auto-mint keyed default" is `getOrInsertWith`.
//
// Fine-grained, not coarse: each entry is its own reactive node, so a reader of
// entry `a` is not invalidated when entry `b` changes; membership (the set of
// keys) is tracked by a dedicated version cell, so `keys` / `len` readers
// recompute only when keys are added or removed, and a pure reorder invalidates
// only order readers.
//
// Rust reference: `lazily-rs/src/cell_family.rs`.

/**
 * Which kind of reactive node a {@link ReactiveMap} entry is — the handle-kind
 * axis the map abstracts over. Mirrors `EntryKind` in `lazily-formal`.
 * @enum {string}
 */
export const EntryKind = Object.freeze({
  /** An input cell — always materialized on `get`. */
  Cell: "cell",
  /** A derived slot — materialized eagerly (pre-mint) or lazily on first read. */
  Slot: "slot",
});

/**
 * A keyed reactive collection generic over the entry handle kind: a map of
 * `K -> handle` with reactive membership and independently-tracked per-entry
 * nodes.
 *
 * Operations run against the owning `Context` (from `./reactive.js`), like the
 * rest of `lazily`. The two specializations a binding exposes are {@link CellMap}
 * (input cells) and {@link SlotMap} (derived slots).
 *
 * @template K, V
 */
export class ReactiveMap {
  /** @type {import("./reactive.js").Context} */
  _ctx;
  /** @type {EntryKind} */
  _kind;
  /** Per-key reactive node handles: key -> handle. @type {Map<K, any>} */
  _entries = new Map();
  /** Insertion-ordered authoritative key list. @type {K[]} */
  _order = [];
  /** Reactive set-membership signal; bumped only when the key set changes. */
  _membership;
  /** Untracked mirror of the membership version. @type {number} */
  _version = 0;
  /** Reactive order signal; bumped on add/remove AND on move/reorder. */
  _orderSignal;
  /** Untracked mirror of the order version. @type {number} */
  _orderVersion = 0;

  /**
   * @param {import("./reactive.js").Context} ctx owning context
   * @param {EntryKind} [kind] entry handle kind; defaults to {@link EntryKind.Slot}
   */
  constructor(ctx, kind = EntryKind.Slot) {
    if (kind !== EntryKind.Cell && kind !== EntryKind.Slot) {
      throw new TypeError("kind must be EntryKind.Cell or EntryKind.Slot");
    }
    this._ctx = ctx;
    this._kind = kind;
    this._membership = ctx.source(0);
    this._orderSignal = ctx.source(0);
  }

  /** Bump the order signal (invalidates `keys` readers). */
  _bumpOrder() {
    this._orderVersion = (this._orderVersion + 1) >>> 0;
    this._ctx.set(this._orderSignal, this._orderVersion);
  }

  /** Bump set-membership (invalidates `len`/`containsKey` readers) + order. */
  _bumpMembership() {
    this._version = (this._version + 1) >>> 0;
    this._ctx.set(this._membership, this._version);
    // The key set changed, so the ordered key list changed too.
    this._bumpOrder();
  }

  /**
   * Mint the entry node for `key` (via `compute` as its canonical value
   * producer) on first access, caching the handle and bumping reactive
   * membership. Re-minting an existing key returns the cached handle.
   * @param {K} key
   * @param {() => V} compute
   * @returns {any} the entry handle
   */
  _mint(key, compute) {
    const existing = this._entries.get(key);
    if (existing !== undefined) {
      return existing; // warm: already allocated.
    }
    // An input cell sets its value directly; a derived slot wraps `compute` as
    // its recomputation — the same node an eager pre-mint would allocate.
    const handle =
      this._kind === EntryKind.Cell ? this._ctx.source(compute()) : this._ctx.computed(() => compute());
    this._entries.set(key, handle);
    this._order.push(key);
    this._bumpMembership();
    return handle;
  }

  /**
   * Read a handle's value through the given reactive surface `ops` (subscribes
   * the caller when `ops` is the value-threaded `Compute` view; an untracked read
   * when `ops` is the bare `Context`). #lzcellkernel: tracking is value-threaded,
   * so the caller passes the compute view it received to subscribe.
   */
  _observe(ops, handle) {
    return ops.get(handle);
  }

  /**
   * Get the value at `key`, minting the entry via `factory(key)` first if the
   * key is absent — the mint-on-access recipe. For a {@link SlotMap} this is the
   * LAZY materialization pull; for a {@link CellMap} it seeds an input cell.
   * Bumps reactive membership only on insert; an existing key returns its
   * current value without re-running the factory.
   * @param {import("./reactive.js").ComputeOps} ops reactive surface (the
   *   `Compute` view inside a compute/effect closure, else the owning `Context`)
   * @param {K} key
   * @param {(key: K) => V} factory
   * @returns {V}
   */
  getOrInsertWith(ops, key, factory) {
    const existing = this._entries.get(key);
    if (existing !== undefined) {
      return this._observe(ops, existing);
    }
    const handle = this._mint(key, () => factory(key));
    return this._observe(ops, handle);
  }

  /**
   * Return the existing entry handle for `key`, or `undefined`. Non-reactive.
   * @param {K} key
   * @returns {any}
   */
  handle(key) {
    return this._entries.get(key);
  }

  /**
   * Read the value at `key` if present, else `undefined`. Reactive on that entry
   * only (a reader is invalidated when this entry changes, not when siblings do).
   * @param {import("./reactive.js").ComputeOps} ops reactive surface (the
   *   `Compute` view inside a closure, else the owning `Context`)
   * @param {K} key
   * @returns {V | undefined}
   */
  get(ops, key) {
    const handle = this._entries.get(key);
    return handle === undefined ? undefined : this._observe(ops, handle);
  }

  /**
   * Remove `key`'s entry. Bumps reactive membership. Returns whether the key was
   * present. (The underlying node id is not recycled; the orphaned node stops
   * being referenced by the map.)
   * @param {K} key
   * @returns {boolean}
   */
  remove(key) {
    if (!this._entries.has(key)) {
      return false;
    }
    this._entries.delete(key);
    const idx = this._order.indexOf(key);
    if (idx !== -1) {
      this._order.splice(idx, 1);
    }
    this._bumpMembership();
    return true;
  }

  /**
   * Reactive snapshot of the keys in their current order. Subscribes the caller
   * to ORDER changes (add/remove AND move/reorder), not to per-entry value
   * changes.
   * @param {import("./reactive.js").ComputeOps} ops reactive surface (the
   *   `Compute` view inside a closure, else the owning `Context`)
   * @returns {K[]}
   */
  keys(ops) {
    ops.get(this._orderSignal);
    return [...this._order];
  }

  /**
   * The currently-materialized (present) keys, in first-materialization order.
   * Non-reactive; the present set only grows (deferral, not de-allocation).
   * @returns {K[]}
   */
  presentKeys() {
    return [...this._order];
  }

  /** Number of currently-materialized (present) entries. Non-reactive. */
  presentCount() {
    return this._order.length;
  }

  /** Whether `key` is currently materialized (present). Non-reactive. */
  isPresent(key) {
    return this._entries.has(key);
  }

  /**
   * Current 0-based position of `key` in the order, or `undefined` if absent.
   * Non-reactive.
   * @param {K} key
   * @returns {number | undefined}
   */
  position(key) {
    const i = this._order.indexOf(key);
    return i === -1 ? undefined : i;
  }

  /**
   * Atomically move `key` to `index` in the order (`#lzcellmove`). The entry
   * keeps the SAME node, dependents, and lineage — unlike `remove` + re-mint.
   * Only the order signal is bumped (once), so `keys` readers recompute but
   * `len`/`containsKey` readers stay cached. `index` is clamped to `[0, len)`.
   * Returns whether `key` was present.
   * @param {K} key
   * @param {number} index
   * @returns {boolean}
   */
  moveTo(key, index) {
    const from = this._order.indexOf(key);
    if (from === -1) {
      return false;
    }
    const to = Math.min(index, Math.max(this._order.length - 1, 0));
    if (from === to) {
      return true; // no-op: do not invalidate readers needlessly.
    }
    this._order.splice(from, 1);
    this._order.splice(to, 0, key);
    this._bumpOrder();
    return true;
  }

  /**
   * Atomically move `key` to just before `anchor` in the order (`#lzcellmove`).
   * No-op returns `false` if either key is absent.
   * @param {K} key
   * @param {K} anchor
   * @returns {boolean}
   */
  moveBefore(key, anchor) {
    const anchorIdx = this.position(anchor);
    if (anchorIdx === undefined) {
      return false;
    }
    const from = this.position(key);
    if (from === undefined) {
      return false;
    }
    // Removing `key` first shifts `anchor` left by one when key precedes it.
    const target = from < anchorIdx ? anchorIdx - 1 : anchorIdx;
    return this.moveTo(key, target);
  }

  /**
   * Atomically move `key` to just after `anchor` in the order (`#lzcellmove`).
   * @param {K} key
   * @param {K} anchor
   * @returns {boolean}
   */
  moveAfter(key, anchor) {
    const anchorIdx = this.position(anchor);
    if (anchorIdx === undefined) {
      return false;
    }
    const from = this.position(key);
    if (from === undefined) {
      return false;
    }
    const target = from <= anchorIdx ? anchorIdx : anchorIdx + 1;
    return this.moveTo(key, target);
  }

  /**
   * Reactive entry count. Subscribes the caller to membership changes only.
   * @param {import("./reactive.js").ComputeOps} ops reactive surface (the
   *   `Compute` view inside a closure, else the owning `Context`)
   */
  len(ops) {
    ops.get(this._membership);
    return this._order.length;
  }

  /**
   * Reactive emptiness check. Subscribes the caller to membership changes.
   * @param {import("./reactive.js").ComputeOps} ops reactive surface
   */
  isEmpty(ops) {
    return this.len(ops) === 0;
  }

  /**
   * Reactive membership test for `key`. Subscribes the caller to membership
   * changes (add/remove of any key), not to value changes.
   * @param {import("./reactive.js").ComputeOps} ops reactive surface (the
   *   `Compute` view inside a closure, else the owning `Context`)
   * @param {K} key
   * @returns {boolean}
   */
  containsKey(ops, key) {
    ops.get(this._membership);
    return this._entries.has(key);
  }

  /** Non-reactive count. Does not subscribe the caller to anything. */
  lenUntracked() {
    return this._order.length;
  }

  /** This map's entry kind ({@link EntryKind.Cell} or {@link EntryKind.Slot}). */
  entryKind() {
    return this._kind;
  }
}

/**
 * A keyed INPUT-CELL collection: every entry is a settable input cell. The
 * `CellMap` specialization of {@link ReactiveMap} adds cell-only `set` and eager
 * value-minting (`entry` / `entryWith`) on top of the shared reactive keyed
 * surface.
 *
 * @template K, V
 * @extends {ReactiveMap<K, V>}
 */
export class CellMap extends ReactiveMap {
  /** @param {import("./reactive.js").Context} ctx */
  constructor(ctx) {
    super(ctx, EntryKind.Cell);
  }

  /**
   * Return the value cell for `key`, minting it with `default` (computed via the
   * closure) on first access. Subsequent calls return the cached handle. Adding
   * a new key bumps reactive membership; re-fetching an existing key does not.
   * Cell-only: eager value-minting has no derived-slot analog.
   * @param {K} key
   * @param {() => V} defaultFn
   * @returns {import("./reactive.js").CellHandle<V>}
   */
  entryWith(key, defaultFn) {
    const existing = this._entries.get(key);
    if (existing !== undefined) {
      return existing;
    }
    const value = defaultFn();
    return this._mint(key, () => value);
  }

  /**
   * Return the value cell for `key`, minting it with `default` on first access.
   * Convenience wrapper over {@link CellMap#entryWith}.
   * @param {K} key
   * @param {V} defaultValue
   * @returns {import("./reactive.js").CellHandle<V>}
   */
  entry(key, defaultValue) {
    return this.entryWith(key, () => defaultValue);
  }

  /**
   * Set the value at `key`, inserting a new entry (and bumping membership) if it
   * does not exist yet. Updating an existing entry leaves membership untouched
   * and invalidates only that entry's dependents. Cell-only: an input is
   * settable; a derived {@link SlotMap} slot is not.
   * @param {K} key
   * @param {V} value
   */
  set(key, value) {
    const existing = this._entries.get(key);
    if (existing !== undefined) {
      this._ctx.set(existing, value);
      return;
    }
    this.entryWith(key, () => value);
  }
}

/**
 * A keyed DERIVED-SLOT collection: every entry is a derived slot whose value is
 * derived. `getOrInsertWith` mints a slot on first access (lazy
 * materialization); {@link SlotMap#materializeAll} pre-mints the keyset (eager).
 * A slot's value is derived, so `SlotMap` has NO `set`.
 *
 * @template K, V
 * @extends {ReactiveMap<K, V>}
 */
export class SlotMap extends ReactiveMap {
  /** @param {import("./reactive.js").Context} ctx */
  constructor(ctx) {
    super(ctx, EntryKind.Slot);
  }

  /**
   * EAGER materialization: pre-mint a derived slot for every key in `keys` via
   * `factory`, up front. Observationally identical to minting each key lazily on
   * first read — it only changes WHEN the nodes are allocated.
   * @param {Iterable<K>} keys
   * @param {(key: K) => V} factory
   */
  materializeAll(keys, factory) {
    // Eager pre-mint runs at top level (not inside a compute), so the owning
    // context is the reactive surface — an untracked observe that only forces
    // allocation. #lzcellkernel: reads are value-threaded via `ops`.
    for (const key of keys) {
      this.getOrInsertWith(this._ctx, key, factory);
    }
  }
}
