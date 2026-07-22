// Reactive dependency graph (lazily-spec/docs/reactive-graph.md) — the native
// JavaScript counterpart of lazily-kt's `Context` and lazily-rs's `Context`.
//
// The reactive family is the Cell kernel (#lzcellkernel), naming v2: `Cell` is
// the value-node CONCEPT over two value kinds — `Source` (a value written from
// outside; `source`) and `Computed` (a value computed from upstream; `computed`,
// always guarded) — plus `Effect` (a value-less side-effecting sink). Reading a
// cell inside a computation auto-registers a dependency edge; writing a source
// invalidates dependents. See tasks/software/lazily-cell-kernel-design.md +
// naming-spec-v2.
//
// - Lazy computeds mark dirty on invalidation and recompute on the next read
//   (pull-based, glitch-free: a computed always observes consistent inputs).
// - Source cells use a `==` (PartialEq) guard: setting an equal value is a no-op.
// - Every `computed` is guarded: a `==` guard so an equal recompute suppresses
//   downstream (matches TC39 `Signal.Computed`). There is no unguarded mode.
// - Eager = an EAGER computed (`computed(f).eager()`): a puller effect keeps it
//   materialized by the time the invalidating `set`/`batch` returns. Eagerness
//   is graph state (the F_EAGER bit + `eagerBy` side table), not a kind, so the
//   former `Signal` is retired and the #lzsignaleager puller bug cannot be
//   written.
// - Effects rerun after any tracked dependency invalidates.
//
// The read/write split has no compile-time (or, by design §4, runtime) gate in
// JavaScript; it is expressed by METHOD PRESENCE — `Source` has `set`/`merge`,
// `Computed` does not. The v1 handle names (`SourceCell`/`FormulaCell`,
// `CellHandle`/`SlotHandle`) are retired; `signal` and `cell`/`slot`
// constructors remain as deprecated aliases.
//
// #lzjsclosure: this module uses the closure factory technique (rmemo-style)
// rather than `class` + `#private` fields. `createContext()` returns an object
// whose methods close over captured `let` state; nodes are plain objects with
// a numeric discriminator (KIND_CELL/KIND_SLOT/KIND_EFFECT) replacing
// `instanceof`. V8 inlines the small monomorphic closures more aggressively
// than prototype methods touching `#private` fields, so the read/invalidate
// hot paths are 2-8x faster than the prior class implementation (see
// bench/closure-vs-class.bench.mjs) while also shaving ~8% off the
// minified+brotlied payload.
//
// #lzjsarenanodes: the hot per-node scalar state (kind + packed boolean flags)
// lives in two arena typed arrays (`kinds` / `flags`) keyed by id, not on the
// node objects themselves. The `nodes` array keeps only non-scalar fields
// (value, compute fn, edge arrays), shrinking each node object from up to 10
// properties to ≤4. That lets V8 emit one stable hidden class per shape and
// trims per-node memory from ~300 B to ~150 B on the spreadsheet-scale
// workloads in BENCHMARKS.md. Boolean fields are bit-packed: hasValue/memo/
// dirty/forceRecompute/inProgress (slots) and forceRun (effects).

import { TeardownScope } from "./teardown-scope.js";

export function defaultEqual(a, b) {
  if (a === b) {
    return true;
  }
  if (a === null || b === null || typeof a !== "object" || typeof b !== "object") {
    return false;
  }
  if (a instanceof Uint8Array && b instanceof Uint8Array) {
    if (a.length !== b.length) {
      return false;
    }
    for (let i = 0; i < a.length; i++) {
      if (a[i] !== b[i]) {
        return false;
      }
    }
    return true;
  }
  // #lzjsshalloweq: fast path for plain arrays — Array.isArray + length check
  // before Object.keys, index loop, no closure allocation.
  if (Array.isArray(a) && Array.isArray(b)) {
    const n = a.length;
    if (n !== b.length) {
      return false;
    }
    for (let i = 0; i < n; i++) {
      if (!defaultEqual(a[i], b[i])) {
        return false;
      }
    }
    return true;
  }
  const aKeys = Object.keys(a);
  const bKeys = Object.keys(b);
  if (aKeys.length !== bKeys.length) {
    return false;
  }
  for (let i = 0; i < aKeys.length; i++) {
    const k = aKeys[i];
    if (!(k in b) || !defaultEqual(a[k], b[k])) {
      return false;
    }
  }
  return true;
}

/**
 * Thrown when a node that has been torn down is read (`read_after_dispose`).
 *
 * Disposal is not a value. A binding that answers a read on a disposed node with
 * its last-computed value, a zero, or `undefined` makes "torn down"
 * indistinguishable from "legitimately this value", and a use-after-dispose bug
 * then surfaces as a wrong number far from its cause. It is also what lets a
 * caller — an effect body, a teardown path — narrow the failure to *this* cause
 * with a `instanceof` check instead of swallowing every error.
 *
 * The id is carried for diagnostics only. Note that ids are recycled here (see
 * `freeIds`), so this error means "the id currently names no live node", which
 * is the strongest claim a recycling binding can make: once the id is handed to
 * a new node, a stale handle addresses that new node and the corpus's
 * `recycled_id_inherits_nothing` / `dispose_stale_handle` cases govern instead.
 */
export class DisposedNodeError extends Error {
  constructor(id) {
    super(`read after dispose: node ${id} has been disposed`);
    this.name = "DisposedNodeError";
    this.nodeId = id;
  }
}

// -- Handles: the Cell kernel (#lzcellkernel), naming v2 ---------------------
//
// v2 (2026-07-21): the bare kind name is the HANDLE a caller holds; `Cell` is
// the value-node CONCEPT (not a class you construct/hold). Two value handles:
//
//   Cell                    concept — a value-bearing reactive node (not exported)
//   ├─ Source (was SourceCell)   written from outside; folds under a policy
//   └─ Computed (was FormulaCell) computed from upstream (guarded, always)
//
//   Effect (was EffectHandle)    value-less sink — outside the Cell hierarchy
//
// See tasks/software/lazily-cell-kernel-design.md + naming-spec-v2. This is a
// rename over v1's `SourceCell`/`FormulaCell` handles + `Cell` genus base: the
// handles are now `Source`/`Computed`/`Effect`, and `Cell` is an internal,
// unexported shared base embodying the value-node concept (never constructed
// bare). There is no `Reactive` trait/type uniting cells + effects.
//
// JavaScript has no compile-time kind enforcement, so the read/write split is
// expressed by METHOD PRESENCE on the concrete class: a `Source` object
// exposes `set`/`merge`; a `Computed` object does not (reading `computed.set`
// is `undefined`). No runtime gate is invented — design §4 rejected downgrading
// the compile guarantee to a runtime panic, and JS simply has neither. Handles
// created through a context carry a non-enumerable back-reference so the
// instance methods can delegate to the closure API; handles built directly
// (`new Source(id)`) carry none and are used through the functional unified
// `ctx.get(handle)` / `ctx.set(handle, v)` surface (#lzcellkernel).

class Cell {
  /** @internal */ constructor(id, ctx) {
    this.id = id;
    if (ctx !== undefined) {
      Object.defineProperty(this, "_ctx", { value: ctx });
    }
  }
  /** Tear this node down (kind-agnostic; dispatches on the handle's class). */
  dispose() {
    this._ctx.disposeNode(this);
  }
}

/**
 * A cell written from outside, folding writes under an optional merge policy
 * (default keep-latest = replace). Subsumes the former `MergeCell` wrapper:
 * a keep-latest `Source` ≡ the old plain cell. Exposes `set`/`merge` — this is
 * the writable kind (v1 `SourceCell`).
 */
export class Source extends Cell {
  /** @internal */ constructor(id, ctx, policy) {
    super(id, ctx);
    if (policy !== undefined && policy !== null) {
      this.policy = policy;
    }
    Object.freeze(this);
  }
  /**
   * Read the current value. Pass the {@link Compute} view (`cx`) a
   * compute/effect closure received to REGISTER a dependency edge against the
   * recomputing node (#lzcellkernel value-threaded tracking); a bare `get()` at
   * top level — or inside `untracked()` — reads WITHOUT forming an edge. There
   * is no ambient tracking carrier, so an in-closure read must thread `cx`.
   */
  get(cx) {
    return cx !== undefined ? cx.get(this) : this._ctx.get(this);
  }
  /** Replace the value outright (the keep-latest write). */
  set(value) {
    this._ctx.set(this, value);
  }
  /**
   * Fold `op` into the current value under this cell's policy. With no policy
   * (keep-latest) this is a plain replace (keep-latest `Source`), so
   * the `==` store-guard + store-without-cascade apply unchanged.
   */
  merge(op) {
    const policy = this.policy;
    if (policy) {
      this._ctx.set(this, policy.merge(this._ctx.get(this), op));
    } else {
      this._ctx.set(this, op);
    }
  }
}

/**
 * A cell computed from upstream. Always guarded (equality-suppressed): an equal
 * recompute suppresses downstream, matching TC39 `Signal.Computed`. Lazy by
 * default; `computed(f).eager()` makes it eager. Exposes no `set`/`merge` — it
 * is not written from outside (v1 `FormulaCell`).
 */
export class Computed extends Cell {
  /** @internal */ constructor(id, ctx) {
    super(id, ctx);
    Object.freeze(this);
  }
  /**
   * Read the current value. Pass the {@link Compute} view (`cx`) a
   * compute/effect closure received to REGISTER a dependency edge against the
   * recomputing node (#lzcellkernel value-threaded tracking); a bare `get()` at
   * top level — or inside `untracked()` — reads WITHOUT forming an edge. There
   * is no ambient tracking carrier, so an in-closure read must thread `cx`.
   */
  get(cx) {
    return cx !== undefined ? cx.get(this) : this._ctx.get(this);
  }
  /**
   * Make this computed eager: attach a puller `Effect` that keeps it
   * materialized after every invalidation, so the value goes `v1 -> v2` with no
   * intermediate unset state. Idempotent (a second `eager()` is a no-op) and
   * returns the SAME handle (mutated graph state). This is the eager
   * construction that retired the former `Signal` (was `drive()`).
   */
  eager() {
    this._ctx.makeEager(this.id);
    return this;
  }
  /**
   * Reverse of {@link eager}: stop eager recomputation and dispose the puller.
   * The value remains readable and reverts to lazy. No-op if already lazy (was
   * `undrive()`).
   */
  lazy() {
    this._ctx.makeLazy(this.id);
    return this;
  }
  /** Whether this computed is currently eager (has an active puller). */
  isEager() {
    return this._ctx.isEager(this);
  }
}

/** A value-less side-effecting sink. Outside the `Cell` hierarchy (v1 `EffectHandle`). */
export class Effect {
  /** @internal */ constructor(id, ctx) {
    this.id = id;
    if (ctx !== undefined) {
      Object.defineProperty(this, "_ctx", { value: ctx });
    }
    Object.freeze(this);
  }
  /** Dispose this effect (unsubscribe). */
  dispose() {
    this._ctx.disposeEffect(this);
  }
}

/**
 * @deprecated Retired by the Cell kernel (#lzcellkernel): the eager construction
 * is now `ctx.computed(f).eager()`. Retained as a compatibility shape for the
 * thread-safe / async contexts (which keep their own signal handles for now,
 * mirroring lazily-rs) and the `state-machine` helper.
 */
export class SignalHandle {
  /** @internal */ constructor(slot, effect) {
    this.slot = slot;
    this.effect = effect;
    Object.freeze(this);
  }
}

// v2 retired the v1 handle aliases `CellHandle`/`SlotHandle` (and the earlier
// `SourceCell`/`FormulaCell` handle names). The canonical handles are `Source`
// / `Computed` / `Effect`. The `.d.ts` keeps type-only `CellHandle`/`SlotHandle`
// aliases so peripheral modules' declaration files still resolve during the
// staged family-wide rename; no runtime alias is exported.

// -- Node discriminators (replace `instanceof` in hot loops) ----------------
// #lzjsarenanodes: the kind lives in the `kinds` Uint8Array arena (one byte
// per id), not on the node object. KIND_NONE (0) marks a free id — it is what
// `kinds[id]` reads as for never-allocated ids (typed arrays zero-initialize)
// and what dispose* explicitly writes back when recycling an id. The node
// objects in `nodes[]` no longer carry a `k` field.

const KIND_NONE = 0;
const KIND_CELL = 1;
const KIND_SLOT = 2;
const KIND_EFFECT = 3;

// Packed boolean flags stored in the `flags` Uint8Array arena (one byte per
// id). Slot nodes use the first five bits; Effect nodes use F_FORCE_RUN. Cell
// nodes carry no flags. Packing the booleans into one byte/id (vs ~5 separate
// properties × ~8 B each on the slot object) is most of the #lzjsarenanodes
// per-node memory win.
const F_HAS_VALUE = 1 << 0;        // slot: a memoized value is cached
const F_MEMO = 1 << 1;             // slot: equality-suppressed recompute
const F_DIRTY = 1 << 2;            // slot: invalidate-on-next-read marker
const F_FORCE_RECOMPUTE = 1 << 3;  // slot: force a recompute even if not dirty
const F_IN_PROGRESS = 1 << 4;      // slot: cycle-detection tripwire
const F_FORCE_RUN = 1 << 5;        // effect: force the next run regardless
const F_EAGER = 1 << 6;            // slot: this computed is eager (has a puller)

// #lzspecedgeindex: width at which an edge list promotes from linear-scan dedup
// to a hash index. Measured, not inherited — see the note on `edgeInsert`.
//
// Below this width the linear scan over a small packed SMI array is faster than
// one Map lookup; above it the scan's O(n^2/2) total comparisons dominate. The
// crossover for the structure actually used here (Map<id, position>) measured at
// width 160 on node v26 (map/linear = 0.77x at 160, 1.03x — break-even — at 128).
// A Set crosses over earlier (~96) but cannot give O(1) *removal*, which the
// recompute path needs, so Map<id, position> is the structure and 160 is its
// crossover.
//
// This is 5x the `32` used elsewhere in the lazily family: at width 32 the hash
// index measured 2.3x SLOWER than the scan here, so importing that constant
// would have made every mid-degree node slower than before the fix. The spec is
// explicit that this threshold is not portable (`#lzspecedgeindex`).
const EDGE_INDEX_PROMOTE = 160;

// Dedup an edge list, promoting to a hash index past EDGE_INDEX_PROMOTE.
//
// `index` is a side table `Map<ownerId, Map<edgeId, position>>`; `ownerId` keys
// the list being mutated. The inner map holds each edge's *position* in `edges`,
// not just membership, so `edgeRemoveIndexed` can swap-remove in O(1) instead of
// rescanning — the dependents list of a wide source is rebuilt one edge at a
// time on every dependent recompute, so an O(n) removal would reintroduce the
// same O(n^2) on the recompute path that the index removes from the build path.
//
// Promotion is one-way: there is no demotion. A shrinking list keeps its index.
// This is deliberate — edges are removed and re-registered on every recompute,
// so a list sitting near the boundary oscillates by one, and a shared
// promote/demote boundary would rebuild the index on every recompute. The spec
// permits "demote well below the promote threshold, or do not demote at all";
// not demoting is the variant with no thrash window at all. The index is instead
// dropped wholesale when the list is cleared or its owner is torn down, which is
// also what keeps a recycled id from inheriting a stale index.
function edgeInsertIndexed(edges, id, ownerId, index) {
  const pos = index.get(ownerId);
  if (pos !== undefined) {
    if (pos.has(id)) {
      return false;
    }
    pos.set(id, edges.length);
    edges.push(id);
    return true;
  }
  const n = edges.length;
  if (n < EDGE_INDEX_PROMOTE) {
    for (let i = 0; i < n; i++) {
      if (edges[i] === id) {
        return false;
      }
    }
    edges.push(id);
    return true;
  }
  // Cross the threshold: build the index once, then take the indexed path.
  const built = new Map();
  for (let i = 0; i < n; i++) {
    built.set(edges[i], i);
  }
  index.set(ownerId, built);
  if (built.has(id)) {
    return false;
  }
  built.set(id, n);
  edges.push(id);
  return true;
}

function edgeRemoveIndexed(edges, id, ownerId, index) {
  const pos = index.get(ownerId);
  if (pos === undefined) {
    // Narrow list: linear swap-remove, unchanged from the pre-index behavior.
    for (let i = 0; i < edges.length; i++) {
      if (edges[i] === id) {
        edges[i] = edges[edges.length - 1];
        edges.pop();
        return true;
      }
    }
    return false;
  }
  const at = pos.get(id);
  if (at === undefined) {
    return false;
  }
  // Swap-remove, keeping the index's positions consistent with the array.
  const last = edges.length - 1;
  const moved = edges[last];
  edges[at] = moved;
  edges.pop();
  pos.delete(id);
  if (moved !== id) {
    pos.set(moved, at);
  }
  return true;
}

/**
 * Create a reactive {@link Context} — the idiomatic entry point.
 *
 * Implemented with the closure factory technique (#lzjsclosure): graph state is
 * captured in closure bindings rather than class instance fields, so V8 inlines
 * the hot paths more aggressively than the prior `class` + `#private` version.
 *
 * `Context` is an alias of this function (same binding), so the historical
 * `new Context(opts)` call sites keep working unchanged. A function that returns
 * an object is a legal constructor under the JS spec — `new` yields the returned
 * object — so both `createContext(opts)` and `new Context(opts)` produce the
 * same reactive context.
 *
 * @param {{ instrument?: boolean }} [opts] pass `{ instrument: true }` to
 *   accumulate reactive-core counters readable via
 *   {@link Context.instrumentationSnapshot}.
 */
function createContext(opts = {}) {
  const instrument = !!(opts && opts.instrument === true);
  const nodes = [];
  // #lzjsarenanodes: arena typed arrays keyed by id. `kinds[id]` holds the
  // KIND_* discriminator (replacing the per-node `k` field); `flags[id]` holds
  // the packed F_* boolean bits (replacing hasValue/dirty/forceRecompute/
  // inProgress/memo/forceRun). They start small and double on demand via
  // ensureCapacity; both are `let` so growth can rebind them — every closure
  // sees the latest binding. 2 bytes/id total versus ~50 B/id for the same
  // fields as object properties is the bulk of the per-node savings.
  let kinds = new Uint8Array(32);
  let flags = new Uint8Array(32);
  let nextId = 1;
  const freeIds = [];
  // #lzspecedgeindex: hash indexes for wide edge lists, keyed by the id of the
  // node that owns the list. Held outside the node objects on purpose — a node
  // whose degree stays below EDGE_INDEX_PROMOTE (the overwhelming majority, and
  // every node in a wide fan-out except the source itself) has no entry here and
  // pays nothing, not even a null field slot. That keeps the #lzjsarenanodes
  // per-node footprint intact: scalar state stays in the typed-array arenas,
  // variable-size edge state stays lazily allocated.
  //
  // Because ids are recycled (`freeIds`), every teardown path MUST drop the
  // entries for the id it frees, or a new node would alias the previous
  // occupant's index. See disposeSlot/disposeCell/disposeEffect.
  const dependentsIndex = new Map();
  const dependenciesIndex = new Map();
  // #lzcellkernel: eager-computed side table (computed id -> puller effect id).
  // Eagerness is graph state — the F_EAGER bit on the computed's node plus this
  // owner-keyed table, cleared on dispose/`.lazy()`. One entry per EAGER computed,
  // zero per lazy one (the EdgeIndex precedent: per-node bit for the common case,
  // a side table for the rare one). Because ids are recycled, every teardown of an
  // eager computed MUST drop its entry or a recycled id would alias a stale one.
  const eagerBy = new Map();
  // #lzcellkernel: custom propagate-guard side table (slot id -> equals fn).
  // A guarded computed normally suppresses on `defaultEqual`; `computedRippleWhen`
  // installs a custom `equals(old, new)` (true = equal = SUPPRESS, the negation of
  // the public `changed` predicate) here. Same rare-case-side-table precedent as
  // `eagerBy`: zero entries for an ordinary `computed`, one per custom-guard slot,
  // and — because ids are recycled — every teardown MUST drop its entry or a
  // recycled id would inherit a stale guard.
  const slotEquals = new Map();
  // #lzcellkernel: the value-threaded `Compute` view is the LITERAL sole
  // tracking surface (`readTracked` / `readTrackedId`). There is NO ambient
  // tracking carrier: a dependency edge is registered only when a read threads
  // the recomputing node's id as a value (the `Compute` view carries it in
  // `_slotId`). A bare `handle.get()` / `ctx.get(handle)` at top level — and any
  // read that does not thread the view — forms no edge. This matches the sibling
  // static bindings (lazily-cpp / lazily-go / lazily-dart), which all deleted
  // their ambient recompute stack. The thread-safe / async contexts keep their
  // own separate scoping paths.
  let pendingEffects = [];
  let pendingHead = 0;
  const scheduledEffects = new Set();
  let flushingEffects = false;
  let batchDepth = 0;
  let batchedCells = new Set();
  // Reusable scratch buffers for markFrontier (#lzjsscratchfrontier): avoid
  // per-invalidation array allocation by clearing in place. Safe under
  // reentrancy because callers fully consume the returned effects array before
  // flushEffects can re-enter markFrontier (guarded by flushingEffects).
  const frontierEffects = [];
  const frontierStack = [];
  const frontierForceStack = [];
  // #lzspecedgeindex: depth of the disposal-driven invalidation cascade.
  //
  // Non-zero only while `invalidateDisposedDependents` is walking the cone left
  // behind by a teardown. While it is set the walk is MARK-ONLY: `scheduleEffect`
  // drops every effect it reaches. Disposal is not a publish — running an effect
  // here re-enters a body that reads the node currently being torn down, turning
  // `dispose` itself into a throw and breaking the idempotence teardown paths
  // depend on. The contract is "errors on the next recompute", and that recompute
  // is driven by a real write.
  //
  // It is a counter, not a flag, because scope teardown disposes N nodes and each
  // one cascades; a flag would be cleared by the first inner completion.
  let disposalDepth = 0;
  // Opt-in instrumentation (off by default → zero steady-state overhead, so the
  // committed BENCHMARKS.md numbers are unperturbed). Mirrors the counter subset
  // of lazily-rs's `InstrumentationSnapshot` that is meaningful single-threaded.
  let counters = instrument ? zeroCounters() : null;

  function zeroCounters() {
    return {
      nodeAllocations: 0,
      slotRecomputes: 0,
      dependencyEdgesAdded: 0,
      dependencyEdgesRemoved: 0,
      effectQueuePushes: 0,
      maxEffectQueueDepth: 0,
    };
  }

  // #lzjsarenanodes: grow the arena typed arrays when an id falls outside the
  // current capacity. Inlined into allocId (the sole caller) so the hot path
  // stays a single `id < kinds.length` branch; doubling keeps growth amortized
  // O(1) per alloc. `kinds`/`flags` are `let` so the new arrays are visible to
  // every closure that reads them.
  function allocId() {
    if (instrument) counters.nodeAllocations++;
    const id = freeIds.pop() ?? nextId++;
    if (id >= kinds.length) {
      const newCap = kinds.length * 2;
      const newKinds = new Uint8Array(newCap);
      newKinds.set(kinds);
      kinds = newKinds;
      const newFlags = new Uint8Array(newCap);
      newFlags.set(flags);
      flags = newFlags;
    }
    return id;
  }

  // -- Creation ----------------------------------------------------------

  // #lzcellkernel constructor surface (v2): `source` / `computed` / `.eager()`.
  //
  // `source(v)` (keep-latest) is the writable cell; `source(v, policy)` folds
  // under a merge policy. `computed(f)` is GUARDED, ALWAYS (v2: every cell is
  // guarded — an equal recompute suppresses downstream; matches TC39
  // `Signal.Computed`). `memo` is removed (folded into guarded `computed`), and
  // the v1 unguarded `computed` is gone — there is no unguarded mode. The eager
  // construction is `computed(f).eager()`. `cell`/`slot` remain as deprecated
  // aliases (`cell` -> `source`, `slot` -> `computed`) during the staged rename.

  function source(value, policy) {
    return new Source(cellAny(value), api, policy);
  }

  /** @deprecated use {@link source}. */
  function cell(value) {
    return new Source(cellAny(value), api);
  }

  function cellAny(value) {
    const id = allocId();
    // #lzjsarenanodes: kind + (no) flags live in the arena; the node object
    // holds only value + the lazy dependents edge list.
    kinds[id] = KIND_CELL;
    flags[id] = 0;
    nodes[id] = { value, dependents: null }; // #lzjslazyedges
    return id;
  }

  /**
   * A computed cell (guarded, always): an equal recompute suppresses downstream
   * invalidation. The #lzcellkernel v2 derived construction. Lazy by default;
   * `computed(f).eager()` makes it eager.
   */
  function computed(compute) {
    return new Computed(slotAny(compute), api);
  }

  /** @deprecated use {@link computed} (guarded, the only derived construction). */
  function slot(compute) {
    return computed(compute);
  }

  /**
   * A **guarded computed** with an explicit change predicate (#lzcellkernel).
   *
   * Like {@link computed}, but downstream propagation is gated by
   * `changed(old, new)` instead of the value's natural {@link defaultEqual}:
   * `changed` returns `true` to **propagate** the recompute downstream, `false`
   * to **suppress** it (treat it as "no meaningful change"). So:
   *
   * - `computed(f)` ≡ `computedRippleWhen(f, (o, n) => !defaultEqual(o, n))`
   *   (the binding's natural equality — how a `computed` guards). For primitives
   *   that is exactly `(o, n) => o !== n`.
   * - a **pass-through** slot (always propagate, no suppression) is
   *   `computedRippleWhen(f, () => true)`. NB: the deprecated `slot` alias is
   *   NOT this in lazily-js v2 — it is a guarded `computed` (there is no unguarded
   *   mode); the pass-through construction is the `() => true` predicate here.
   *
   * The value is **ALWAYS computed** — this is a *propagate* guard, not a compute
   * guard: the predicate needs `new`, so `f` runs every refresh; `changed` gates
   * only the downstream cascade.
   *
   * `changed` MUST be a **pure** function of `(old, new)`. Reading value-carried
   * state (a version/counter/sequence embedded in the value) is fine and stays
   * deterministic; capturing external mutable state is NOT — it keys off
   * recompute/read frequency under laziness and breaks determinism.
   */
  function computedRippleWhen(compute, changed) {
    // The internal engine guards on equality (true = equal = suppress); `changed`
    // is its negation (true = propagate). Installed as a per-slot equals override.
    const equals = (old, next) => !changed(old, next);
    return new Computed(slotWithEqualsAny(compute, equals), api);
  }

  // Create a guarded slot whose equality-suppression uses `equals(old, new)`
  // (true = equal = suppress) instead of the default `defaultEqual`. Passing
  // `undefined` yields the ordinary `defaultEqual`-guarded computed.
  function slotWithEqualsAny(compute, equals) {
    const id = slotAny(compute);
    if (equals !== undefined) {
      slotEquals.set(id, equals);
    }
    return id;
  }

  function slotAny(compute) {
    const id = allocId();
    // #lzjsarenanodes: kind + hasValue/dirty/forceRecompute/inProgress all
    // start at 0/false. v2: every computed is guarded, so F_MEMO (the
    // equality-suppression bit) is set unconditionally at creation. The node
    // object keeps value (varies), compute (closure), and the two lazy edge
    // lists — down from 10 properties to 4.
    kinds[id] = KIND_SLOT;
    flags[id] = F_MEMO;
    nodes[id] = {
      value: undefined,
      compute,
      dependencies: null, // #lzjslazyedges: allocated on first edge
      dependents: null, // #lzjslazyedges: allocated on first edge
    };
    return id;
  }

  /**
   * @deprecated The eager construction is now `ctx.computed(compute).eager()`.
   * Retained for the thread-safe / async contexts (which keep their own signal
   * handles for now) and `state-machine`.
   */
  function signal(compute) {
    const slot = slotAny(compute);
    // The puller reads `slot` through the value-threaded compute view handed to
    // its run (`c._slotId` is the puller's own effect id), so the eager edge is
    // registered by construction — no ambient tracking.
    const effect = effectAny((c) => {
      readTrackedId(slot, c._slotId);
      return null;
    });
    return new SignalHandle(new Computed(slot, api), new Effect(effect, api));
  }

  function effect(run) {
    return new Effect(effectAny(run), api);
  }

  function effectAny(run) {
    const id = allocId();
    // #lzjsarenanodes: forceRun starts set (force the initial run on
    // registration); the node object keeps run/cleanup + the lazy edge list.
    kinds[id] = KIND_EFFECT;
    flags[id] = F_FORCE_RUN;
    nodes[id] = {
      run,
      dependencies: null, // #lzjslazyedges
      cleanup: null,
    };
    scheduleEffect(id, false);
    flushEffects();
    return id;
  }

  // -- Read --------------------------------------------------------------

  /**
   * The unified cell read of the Cell kernel (#lzcellkernel). Reads BOTH a
   * `Source` cell (returns its stored value) and a `Computed` cell (recomputes
   * if necessary). Supersedes the deprecated {@link getCell}; dispatches on the
   * node kind so a single call site works for either handle.
   */
  function get(handle) {
    const id = handle.id;
    if (kinds[id] === KIND_CELL) {
      return getCellAny(id);
    }
    return getSlotAny(id);
  }

  /** @deprecated use {@link get} — the unified cell read (#lzcellkernel). */
  function getCell(handle) {
    return getCellAny(handle.id);
  }

  function getSignal(handle) {
    return get(handle.slot);
  }

  function getSlotAny(id) {
    // #lzspecedgeindex: the disposed check comes BEFORE `registerDependency`,
    // deliberately. A reader that hits a torn-down node must not leave an edge
    // pointing at it — `registerDependency` would push the freed id onto the
    // reader's own `dependencies` list, and that dangling half-edge outlives the
    // throw. Failing first keeps the reader's upstream set clean for its next
    // recompute.
    if (kinds[id] === KIND_NONE) {
      throw new DisposedNodeError(id);
    }
    // #lzcellkernel: the top-level unified read is UNTRACKED — no ambient carrier
    // exists, so a bare `get`/`handle.get()` forms no edge. A tracked read goes
    // through the value-threaded `Compute` view (`readTracked`/`readTrackedId`).
    refreshSlot(id);
    // After refreshSlot, F_HAS_VALUE is set iff `id` is a SLOT that has
    // produced a value: non-slot kinds (CELL/EFFECT/NONE) never carry the
    // bit, and refreshSlot sets it on first successful recompute. So this one
    // flag check replaces both the kind check and the old hasValue read on
    // the hot cached path.
    if ((flags[id] & F_HAS_VALUE) === 0) {
      throw new Error(`slot ${id} has no value`);
    }
    return nodes[id].value;
  }

  function getCellAny(id) {
    // See `getSlotAny`: disposed first, before any edge is registered.
    if (kinds[id] === KIND_NONE) {
      throw new DisposedNodeError(id);
    }
    // #lzcellkernel: the top-level cell read is UNTRACKED (see getSlotAny) — no
    // ambient carrier, so a bare read forms no edge; tracked reads thread the view.
    if (kinds[id] !== KIND_CELL) {
      throw new Error(`get_cell on non-cell id ${id}`);
    }
    return nodes[id].value;
  }

  // Value-threaded tracked read (#lzcellkernel fortified compute view): register
  // the dependency against an EXPLICIT recomputing-node id passed as a VALUE.
  // `trackId === 0` (never a live id — `nextId` starts at 1) reads untracked, and
  // there is no ambient carrier to fall back to. This is the read primitive the
  // `Compute` view
  // (and the migrated internal pullers) call, so attribution survives suspension:
  // the id is captured in the view, not looked up from ambient state at read time
  // (the browser has no `AsyncLocalStorage`, so value-threading is the only
  // correct async mechanism).
  //
  // Dispatches on kind like the unified {@link get}: a `Source` cell returns its
  // stored value; a `Computed` slot refreshes and returns its snapshot. Disposed
  // check comes BEFORE `registerDependency` (see getSlotAny) so a read that hits a
  // torn-down node leaves no dangling half-edge.
  function readTracked(handle, trackId) {
    return readTrackedId(handle.id, trackId);
  }

  // The id-based core of {@link readTracked}: the sole value-threaded tracking
  // read. `trackId === 0` reads untracked (never a live id — `nextId` starts at
  // 1). Both the `Compute` view (via `readTracked`) and the internal eager
  // pullers (`signal` / `makeEager`) call this to attribute a read to an EXPLICIT
  // recomputing-node id passed as a VALUE — there is no ambient tracking carrier.
  function readTrackedId(id, trackId) {
    if (kinds[id] === KIND_NONE) {
      throw new DisposedNodeError(id);
    }
    if (trackId !== 0) {
      registerDependency(id, trackId);
    }
    if (kinds[id] === KIND_CELL) {
      return nodes[id].value;
    }
    refreshSlot(id);
    if ((flags[id] & F_HAS_VALUE) === 0) {
      throw new Error(`slot ${id} has no value`);
    }
    return nodes[id].value;
  }

  // -- Write -------------------------------------------------------------

  /**
   * The unified cell write of the Cell kernel (#lzcellkernel). Writes a value
   * to a `Source` cell, superseding the deprecated {@link setCell}. Only a
   * `Source` handle is writable — passing a `Computed` (or any non-source)
   * handle throws (write protection, design §3), mirroring lazily-rs where only
   * `Source` implements `Write`.
   */
  function set(handle, value) {
    const id = handle.id;
    if (kinds[id] === KIND_NONE) {
      throw new DisposedNodeError(id);
    }
    if (kinds[id] !== KIND_CELL) {
      throw new Error(
        `set on a non-source handle (id ${id}); writes require a Source cell`,
      );
    }
    setCellAny(id, value);
  }

  /** @deprecated use {@link set} — the unified cell write (#lzcellkernel). */
  function setCell(handle, value) {
    setCellAny(handle.id, value);
  }

  function setCellAny(id, value) {
    // A write that silently vanishes is the same failure mode as a read that
    // silently returns stale, so a disposed cell rejects writes too.
    if (kinds[id] === KIND_NONE) {
      throw new DisposedNodeError(id);
    }
    if (kinds[id] !== KIND_CELL) {
      throw new Error(`set_cell on non-cell id ${id}`);
    }
    const node = nodes[id];
    if (!defaultEqual(node.value, value)) {
      node.value = value;
      if (batchDepth > 0) {
        batchedCells.add(id);
      } else {
        // Store-without-cascade: the new value is stored above (so a late
        // subscriber reads it glitch-free) and lazy Slot dependents are
        // dirty-marked, but the effect flush runs ONLY when the dependent cone
        // actually contains an Effect. A cell with no active reactor pays no
        // flush — the write side of the merge cost law
        // (relaycell-backpressure-analysis.md §4.0 / §5).
        if (invalidateDependentsNow(id)) {
          flushEffects();
        }
      }
    }
  }

  // -- Batch -------------------------------------------------------------

  function batch(run) {
    batchDepth++;
    try {
      run();
    } finally {
      finishBatch();
    }
  }

  function finishBatch() {
    if (batchDepth <= 0) {
      throw new Error("finishBatch without active batch");
    }
    batchDepth--;
    if (batchDepth === 0) {
      flushBatched();
    }
  }

  function flushBatched() {
    const cells = batchedCells;
    batchedCells = new Set();
    const roots = [];
    for (const id of cells) {
      if (kinds[id] === KIND_CELL) {
        const deps = nodes[id].dependents;
        if (deps !== null) {
          for (let i = 0; i < deps.length; i++) {
            roots.push(deps[i]);
          }
        }
      }
    }
    const effects = markFrontier(roots);
    for (let i = 0; i < effects.length; i++) {
      scheduleEffect(effects[i][0], effects[i][1]);
    }
    flushEffects();
  }

  // -- Dispose -----------------------------------------------------------

  function disposeEffect(handle) {
    disposeEffectAny(handle.id);
  }

  function disposeEffectAny(id) {
    // #lzspecedgeindex: dropping a scheduled effect is a Set delete, not a scan.
    // This used to be `pendingEffects.indexOf(id, pendingHead)` + `splice`, which
    // is O(pending) per dispose. That is free when the queue is drained — the
    // common case, since `flushEffects` resets the array — but a cascade that
    // tears down a cohort from *inside* an effect body runs while the rest of
    // that cohort is still queued behind it, making every dispose scan and shift
    // a full-width live window: O(W^2) per publish. `scheduledEffects` is the
    // authority on whether a queued id should run, so the entry can simply be
    // left in place and skipped at drain time (see `flushEffects`).
    scheduledEffects.delete(id);
    if (kinds[id] !== KIND_EFFECT) {
      return;
    }
    const node = nodes[id];
    // #lzjsarenanodes: clear the arena slots so the recycled id reads as
    // KIND_NONE/flags=0, then drop the object reference so the run/cleanup
    // closures can be GC'd.
    // #lzspecedgeindex: drop the index entries for the recycled id.
    dependentsIndex.delete(id);
    dependenciesIndex.delete(id);
    kinds[id] = KIND_NONE;
    flags[id] = 0;
    nodes[id] = undefined;
    freeIds.push(id);
    const deps = node.dependencies;
    if (deps !== null) {
      for (let i = 0; i < deps.length; i++) {
        removeDependentEdge(deps[i], id);
      }
    }
    if (node.cleanup) {
      node.cleanup();
    }
  }

  function isEffectActive(handle) {
    return kinds[handle.id] === KIND_EFFECT;
  }

  function disposeSignal(handle) {
    disposeEffect(handle.effect);
  }

  function isSignalActive(handle) {
    return isEffectActive(handle.effect);
  }

  // -- Eager computeds (#lzcellkernel eager construction, v2) ------------
  //
  // Eager = an eager `Computed`, not a distinct kind. `makeEager` attaches a
  // puller `Effect` that re-reads (re-materializes) the computed after every
  // invalidation; the puller is a SCHEDULED effect, so N invalidations in a
  // batch coalesce into one rerun — which is exactly why the `#lzsignaleager`
  // per-write-puller bug is structurally unwritable here. Eagerness is graph
  // state: the F_EAGER bit on the computed's node + the `eagerBy` side table,
  // both cleared on `.lazy()`/dispose.

  function makeEager(id) {
    if (kinds[id] !== KIND_SLOT) {
      throw new Error(`eager() on non-computed id ${id}`);
    }
    if ((flags[id] & F_EAGER) !== 0) {
      return; // idempotent: a second eager() is a no-op
    }
    // The puller reads the computed through the value-threaded compute view
    // handed to its run (`c._slotId` is the puller's own effect id), so the eager
    // edge is registered by construction — no ambient tracking.
    const eff = effectAny((c) => {
      readTrackedId(id, c._slotId);
      return null;
    });
    flags[id] |= F_EAGER;
    eagerBy.set(id, eff);
  }

  function makeLazy(id) {
    if (kinds[id] !== KIND_SLOT || (flags[id] & F_EAGER) === 0) {
      return; // no-op if not an eager computed
    }
    const eff = eagerBy.get(id);
    if (eff !== undefined) {
      disposeEffectAny(eff);
    }
    eagerBy.delete(id);
    flags[id] &= ~F_EAGER;
  }

  function isEager(handle) {
    const id = typeof handle === "number" ? handle : handle.id;
    return kinds[id] === KIND_SLOT && (flags[id] & F_EAGER) !== 0;
  }

  // Tear down a lazy derived node (slot/computed/memo). A slot is both a consumer
  // (it has `dependencies` — the cells/slots it reads) and a producer (it has
  // `dependents` — the slots/effects that read it), so both edge sets must be
  // detached: upstream so invalidation no longer reaches this node, downstream so
  // a later rerun of a former dependent never dereferences the freed id (see
  // `removeDependentEdge`, which reads `kinds[depId]`). Safe to call on an
  // already-disposed handle or the wrong kind (no-op).
  function disposeSlot(handle) {
    const id = handle.id;
    if (kinds[id] !== KIND_SLOT) {
      return;
    }
    // #lzcellkernel: an eager computed owns a puller effect — tear it down first,
    // so disposing the computed never strands an orphaned puller (the regression
    // the eager-bit design removes). The computed always knows whether it is
    // eager, so no survivor is left behind.
    if ((flags[id] & F_EAGER) !== 0) {
      const eff = eagerBy.get(id);
      if (eff !== undefined) {
        disposeEffectAny(eff);
      }
      eagerBy.delete(id);
    }
    // #lzcellkernel: drop any custom propagate-guard before the id is recycled,
    // so a reused id never inherits a stale `computedRippleWhen` predicate.
    if (slotEquals.size !== 0) {
      slotEquals.delete(id);
    }
    const node = nodes[id];
    // Dirty the surviving readers BEFORE the edges are detached — once the
    // downstream loop below runs, nothing can reach them again.
    invalidateDisposedDependents(node.dependents);
    const deps = node.dependencies;
    if (deps !== null) {
      for (let i = 0; i < deps.length; i++) {
        removeDependentEdge(deps[i], id);
      }
    }
    const dependents = node.dependents;
    if (dependents !== null) {
      for (let i = 0; i < dependents.length; i++) {
        removeDependencyEdge(dependents[i], id);
      }
    }
    // #lzjsarenanodes: clear arena slots before recycling the id.
    // #lzspecedgeindex: drop both index entries before the id goes back on the
    // free list — a slot owns both an incoming and an outgoing edge list, and a
    // recycled id must not inherit either.
    dependentsIndex.delete(id);
    dependenciesIndex.delete(id);
    kinds[id] = KIND_NONE;
    flags[id] = 0;
    nodes[id] = undefined;
    freeIds.push(id);
  }

  // Tear down a source cell. Cells have no `dependencies` (pure source), so only
  // the downstream edges are detached. Callers must ensure nothing still reads the
  // cell in a live compute — disposing a cell a live slot reads will throw on that
  // slot's next recompute (same contract as disposing any still-referenced node).
  function disposeCell(handle) {
    const id = handle.id;
    if (kinds[id] !== KIND_CELL) {
      return;
    }
    const node = nodes[id];
    // See `disposeSlot`: detaching is not enough, the readers must be dirtied.
    invalidateDisposedDependents(node.dependents);
    const dependents = node.dependents;
    if (dependents !== null) {
      for (let i = 0; i < dependents.length; i++) {
        removeDependencyEdge(dependents[i], id);
      }
    }
    // #lzspecedgeindex: a cell owns only a dependents list, but drop both keys
    // anyway — the id is about to be recycled into a node of any kind.
    dependentsIndex.delete(id);
    dependenciesIndex.delete(id);
    kinds[id] = KIND_NONE;
    flags[id] = 0;
    nodes[id] = undefined;
    freeIds.push(id);
  }

  function isSet(handle) {
    const id = handle.id;
    if (kinds[id] !== KIND_SLOT) {
      return false;
    }
    // hasValue && !dirty, packed.
    return (flags[id] & (F_HAS_VALUE | F_DIRTY)) === F_HAS_VALUE;
  }

  // -- Degree introspection + generic teardown (`#lzspecedgeindex`) ------
  //
  // The introspection surface is deliberately COUNTS, NEVER COLLECTIONS. A
  // caller can assert on graph shape — "this subscribe/unsubscribe cycle left
  // the source's live subscriber count where it started" — without a path to
  // the edge arrays, without being able to mutate them, and without pinning any
  // storage strategy into the public contract. In particular the linear-scan /
  // promoted-`Map` hybrid behind `EDGE_INDEX_PROMOTE` stays an implementation
  // detail, which it must: that threshold is measured per binding and is not
  // portable.
  //
  // `SignalHandle` resolves to its backing memo slot, which is the node that
  // actually carries the edges; the puller effect is addressed via
  // `handle.effect` when a caller wants the sink's degree instead.

  function nodeIdOf(handle) {
    return handle.slot !== undefined ? handle.slot.id : handle.id;
  }

  /**
   * How many nodes currently depend on `handle` — the size of its reverse edge
   * set, and the observable the disposal contract is written against. Returns 0
   * for a disposed node and for effects, which are pure sinks.
   */
  function dependentCount(handle) {
    const id = nodeIdOf(handle);
    const kind = kinds[id];
    if (kind !== KIND_CELL && kind !== KIND_SLOT) {
      return 0;
    }
    const dependents = nodes[id].dependents;
    return dependents === null ? 0 : dependents.length;
  }

  /**
   * How many nodes `handle` currently depends on — the size of its forward edge
   * set. Counterpart to {@link dependentCount}: disposal must detach both
   * directions, and detaching only one leaves a dangling half-edge visible here.
   * Returns 0 for a disposed node and for cells, which are pure sources.
   */
  function dependencyCount(handle) {
    const id = nodeIdOf(handle);
    const kind = kinds[id];
    if (kind !== KIND_SLOT && kind !== KIND_EFFECT) {
      return 0;
    }
    const dependencies = nodes[id].dependencies;
    return dependencies === null ? 0 : dependencies.length;
  }

  /**
   * Whether `handle`'s id currently names no live node.
   *
   * Ids are recycled, so this answers "disposed and not yet reused". Once the id
   * has been handed to a new node this reads `false` and the handle addresses
   * that new node — which is exactly why `disposeNode` dispatches on the
   * handle's own class rather than on the arena's current kind.
   */
  function isNodeDisposed(handle) {
    return kinds[nodeIdOf(handle)] === KIND_NONE;
  }

  /**
   * Tear down whatever kind of node `handle` names.
   *
   * Dispatch is on the HANDLE'S CLASS, not on `kinds[id]`. That is load-bearing
   * under id recycling: a stale `CellHandle` whose id has since been reissued to
   * a slot must be a no-op, and dispatching on the arena would instead tear down
   * the innocent new occupant. Each `dispose*` already re-checks the kind, so
   * routing through them makes the stale-handle case a no-op by construction
   * (`dispose_stale_handle` in the corpus).
   */
  function disposeNode(handle) {
    if (handle instanceof Source) {
      disposeCell(handle);
    } else if (handle instanceof Computed) {
      disposeSlot(handle);
    } else if (handle instanceof Effect) {
      disposeEffect(handle);
    } else if (handle instanceof SignalHandle) {
      disposeSignal(handle);
      disposeSlot(handle.slot);
    } else {
      throw new TypeError("disposeNode: not a lazily node handle");
    }
  }

  // -- Teardown scopes ---------------------------------------------------

  /**
   * Open a {@link TeardownScope}: nodes created through it are disposed when it
   * ends, in reverse creation order.
   *
   * Grouping bounds TEARDOWN, not visibility — a scoped node reads parent-owned
   * and sibling-scope-owned nodes freely, and scoping never restricts what an
   * edge may point at.
   *
   * Prefer {@link withScope} when the scope's lifetime is lexical; reach for the
   * explicit `end()` when it is not.
   */
  function scope() {
    return new TeardownScope(api);
  }

  /**
   * Run `body` with a fresh scope and end it in a `finally`, returning whatever
   * `body` returned. The bracketed form, and the one to reach for by default.
   */
  function withScope(body) {
    const s = new TeardownScope(api);
    try {
      return body(s);
    } finally {
      s.end();
    }
  }

  // -- Instrumentation ---------------------------------------------------

  function instrumentationSnapshot() {
    return counters ? { ...counters } : null;
  }

  function resetInstrumentation() {
    if (counters) {
      counters = zeroCounters();
    }
  }

  // -- Internals: edges --------------------------------------------------

  function registerDependency(depId, parentId) {
    const depKind = kinds[depId];
    if (depKind === KIND_CELL || depKind === KIND_SLOT) {
      const dep = nodes[depId];
      if (dep.dependents === null) {
        dep.dependents = [];
      }
      const added = edgeInsertIndexed(dep.dependents, parentId, depId, dependentsIndex);
      if (instrument && added) {
        counters.dependencyEdgesAdded++;
      }
    }
    const parentKind = kinds[parentId];
    if (parentKind === KIND_SLOT || parentKind === KIND_EFFECT) {
      const parent = nodes[parentId];
      if (parent.dependencies === null) {
        parent.dependencies = [];
      }
      edgeInsertIndexed(parent.dependencies, depId, parentId, dependenciesIndex);
    }
  }

  function removeDependentEdge(depId, parentId) {
    const depKind = kinds[depId];
    if (depKind === KIND_CELL || depKind === KIND_SLOT) {
      const dep = nodes[depId];
      if (dep.dependents === null) {
        return;
      }
      const removed = edgeRemoveIndexed(dep.dependents, parentId, depId, dependentsIndex);
      if (instrument && removed) {
        counters.dependencyEdgesRemoved++;
      }
    }
  }

  // Symmetric to {@link removeDependentEdge}: remove `depId` from
  // `nodes[parentId].dependencies` (the consumer-side edge list). Only SLOT and
  // EFFECT nodes carry a `dependencies` list; cells are pure sources. Used by the
  // downstream teardown in {@link disposeSlot}/{@link disposeCell}.
  function removeDependencyEdge(parentId, depId) {
    const parentKind = kinds[parentId];
    if (parentKind === KIND_NONE) {
      return;
    }
    if (parentKind === KIND_SLOT || parentKind === KIND_EFFECT) {
      const parent = nodes[parentId];
      if (parent.dependencies === null) {
        return;
      }
      const removed = edgeRemoveIndexed(parent.dependencies, depId, parentId, dependenciesIndex);
      if (instrument && removed) {
        counters.dependencyEdgesRemoved++;
      }
    }
  }

  // -- Internals: refresh / recompute (pull-based, glitch-free) ----------

  function refreshSlot(id) {
    if (kinds[id] !== KIND_SLOT) {
      return false;
    }
    const node = nodes[id];
    const f = flags[id];
    // hasValue && !dirty && !forceRecompute → cached, nothing to do.
    if ((f & F_HAS_VALUE) !== 0 && (f & (F_DIRTY | F_FORCE_RECOMPUTE)) === 0) {
      return false;
    }
    if ((f & F_IN_PROGRESS) !== 0) {
      throw new Error(
        `lazily: circular dependency detected at slot ${id}; a computed/memo slot depends on itself`,
      );
    }
    flags[id] = f | F_IN_PROGRESS;
    try {
      let dependencyChanged = false;
      const deps = node.dependencies;
      if (deps !== null) {
        for (let i = 0; i < deps.length; i++) {
          const dep = deps[i];
          if (kinds[dep] === KIND_SLOT && refreshSlot(dep)) {
            dependencyChanged = true;
          }
        }
      }
      const needsRecompute =
        (f & F_HAS_VALUE) === 0 || (f & F_FORCE_RECOMPUTE) !== 0 || dependencyChanged;
      if (!needsRecompute) {
        // Clear dirty/forceRecompute; the finally clears inProgress.
        flags[id] &= ~(F_DIRTY | F_FORCE_RECOMPUTE);
        return false;
      }
      return recomputeSlotNow(id, node, f);
    } finally {
      flags[id] &= ~F_IN_PROGRESS;
    }
  }

  // Suppression test for a guarded computed: the per-slot custom equals from
  // `computedRippleWhen` if one is installed, else the natural `defaultEqual`.
  function slotGuardEqual(id, old, next) {
    if (slotEquals.size !== 0) {
      const eq = slotEquals.get(id);
      if (eq !== undefined) {
        return eq(old, next);
      }
    }
    return defaultEqual(old, next);
  }

  function recomputeSlotNow(id, node, f) {
    if (instrument) {
      counters.slotRecomputes++;
    }
    // #lzjslazyedges: clear in place rather than null + realloc. Toggling the
    // field null↔Array on every recompute makes V8 mark it polymorphic and
    // slows the whole refresh cascade; clearing the backing array keeps the
    // field monomorphic while still reusing it. (The lazy-init win is for nodes
    // that never acquire an edge — those stay null.)
    const oldDeps = node.dependencies;
    if (oldDeps !== null) {
      for (let i = 0; i < oldDeps.length; i++) {
        removeDependentEdge(oldDeps[i], id);
      }
      oldDeps.length = 0;
      // #lzspecedgeindex: the list was cleared wholesale, so its index is now
      // entirely stale. Drop it rather than clear it in place — the edges are
      // about to be re-registered by the tracked compute below, which re-promotes
      // if the new dependency set is still wide.
      if (dependenciesIndex.size !== 0) {
        dependenciesIndex.delete(id);
      }
    }
    // The fortified value-threaded compute view (#lzcellkernel) is the sole
    // tracking surface: its `get` registers each edge against `id` by
    // construction. No ambient frame is pushed — a read that does not thread this
    // view (a bare `handle.get()`) forms no edge, by design.
    const view = makeCompute(id);
    let result;
    try {
      result = node.compute(view);
    } finally {
      view._live = false; // fortification: a read on the stale view now throws
    }
    const isMemo = (f & F_MEMO) !== 0;
    const hadValue = (f & F_HAS_VALUE) !== 0;
    // #lzcellkernel: a `computedRippleWhen` slot carries a custom guard in the
    // `slotEquals` side table (true = equal = suppress); everything else uses the
    // natural `defaultEqual`. Guarded by size so an ordinary computed pays nothing.
    const unchanged =
      isMemo && hadValue && slotGuardEqual(id, node.value, result);
    // Clear dirty/forceRecompute for the next cycle. Use `&=` (not assignment
    // from `f`) so F_IN_PROGRESS — set by the caller and cleared by its finally
    // — stays set for the duration of the recompute (cycle detection).
    flags[id] &= ~(F_DIRTY | F_FORCE_RECOMPUTE);
    if (unchanged) {
      return false;
    }
    node.value = result;
    flags[id] |= F_HAS_VALUE;
    if (hadValue) {
      invalidateDependentsNow(id);
    }
    return hadValue;
  }

  // -- Internals: invalidation propagation ------------------------------

  /**
   * Mark a node's dependent cone dirty. Returns `true` iff at least one Effect
   * was scheduled — a `false` result is the store-without-cascade fast path (no
   * active reactor, so no flush is owed).
   */
  function invalidateDependentsNow(id) {
    const kind = kinds[id];
    let roots;
    if (kind === KIND_CELL || kind === KIND_SLOT) {
      const dependents = nodes[id].dependents;
      if (dependents === null) {
        return false;
      }
      roots = dependents;
    } else {
      return false;
    }
    const effects = markFrontier(roots);
    for (let i = 0; i < effects.length; i++) {
      scheduleEffect(effects[i][0], effects[i][1]);
    }
    return effects.length > 0;
  }

  // Dirty the cone that read a node being disposed (`#lzspecedgeindex`).
  //
  // Detaching the edges is not enough on its own: a dependent that already has a
  // cached value would keep serving it forever, since with its dependency edge
  // gone nothing will ever invalidate it again — not even a later write to the
  // disposed node's own source. The spec requires that reader to error on its
  // next recompute, so the cone must be marked dirty and recompute (and hit the
  // freed id) rather than answer from cache.
  //
  // Effects reached by the walk are deliberately NOT scheduled. Disposal is not
  // a publish: an effect's next recompute is driven by a real write, and running
  // one here would re-enter a compute that reads the node currently being torn
  // down, turning `dispose` itself into a throw and breaking the idempotence
  // teardown paths depend on. Marking dirty is sufficient — the contract is
  // "errors on next recompute", not "errors immediately".
  function invalidateDisposedDependents(dependents) {
    if (dependents === null || dependents.length === 0) {
      return;
    }
    // The suppression is mechanized by `disposalDepth`, not by "this function
    // happens not to call scheduleEffect". The frontier is still walked and the
    // effects it reaches are still offered to `scheduleEffect`, which drops them
    // while the depth is non-zero — so the same guard covers every path that
    // reaches an effect during a teardown, including the nested cascades a scope
    // end produces, and removing the guard is a single-line mutation that a
    // direct test can be written against.
    disposalDepth++;
    try {
      const effects = markFrontier(dependents);
      for (let i = 0; i < effects.length; i++) {
        scheduleEffect(effects[i][0], effects[i][1]);
      }
    } finally {
      disposalDepth--;
    }
    // The frontier is a shared scratch array; drop the effect entries so no later
    // caller can mistake them for its own.
    frontierEffects.length = 0;
  }

  function markFrontier(roots) {
    const effects = frontierEffects;
    const stack = frontierStack;
    const forceStack = frontierForceStack;
    effects.length = 0;
    stack.length = 0;
    forceStack.length = 0;
    for (let i = 0; i < roots.length; i++) {
      stack.push(roots[i]);
      forceStack.push(true);
    }
    while (stack.length > 0) {
      const id = stack.pop();
      const force = forceStack.pop();
      const kind = kinds[id];
      if (kind === KIND_SLOT) {
        // #lzjsarenanodes: read+write the dirty/forceRecompute bits in the
        // flags arena. shouldPropagate mirrors the original `!dirty || (force
        // && !forceRecompute)` predicate; then we set dirty (and
        // forceRecompute when forcing).
        const f = flags[id];
        const isDirty = (f & F_DIRTY) !== 0;
        const isForceRecompute = (f & F_FORCE_RECOMPUTE) !== 0;
        const shouldPropagate = !isDirty || (force && !isForceRecompute);
        let newFlags = f | F_DIRTY;
        if (force) {
          newFlags |= F_FORCE_RECOMPUTE;
        }
        flags[id] = newFlags;
        if (shouldPropagate) {
          const ddeps = nodes[id].dependents;
          if (ddeps !== null) {
            for (let i = 0; i < ddeps.length; i++) {
              stack.push(ddeps[i]);
              forceStack.push(false);
            }
          }
        }
      } else if (kind === KIND_EFFECT) {
        effects.push([id, force]);
      }
    }
    return effects;
  }

  // -- Internals: effect scheduling / flush ------------------------------

  function scheduleEffect(id, force) {
    // #lzspecedgeindex: disposal is not a publish. See `disposalDepth`.
    if (disposalDepth > 0) {
      return;
    }
    if (kinds[id] !== KIND_EFFECT) {
      return;
    }
    if (force) {
      flags[id] |= F_FORCE_RUN;
    }
    // `Set.prototype.add` returns the Set, which is always truthy — this read as
    // a dedup but never was one, so an effect reachable from several changed
    // cells was queued once per path. Harmless in run count (`effectShouldRun`
    // gates the repeats) but it inflates the very queue that dispose and drain
    // walk, so the dedup is now real.
    if (!scheduledEffects.has(id)) {
      scheduledEffects.add(id);
      pendingEffects.push(id);
      if (instrument) {
        counters.effectQueuePushes++;
        const depth = pendingEffects.length - pendingHead;
        if (depth > counters.maxEffectQueueDepth) {
          counters.maxEffectQueueDepth = depth;
        }
      }
    }
  }

  function flushEffects() {
    if (flushingEffects) {
      return;
    }
    flushingEffects = true;
    try {
      while (true) {
        const id = pendingEffects[pendingHead++];
        if (id === undefined) {
          pendingEffects = [];
          pendingHead = 0;
          return;
        }
        // A slot whose effect was disposed since it was queued is left in place
        // rather than spliced out (see `disposeEffect`). `runEffect` already
        // drops it: a disposed id reads back as KIND_NONE, and a recycled id
        // belongs to a fresh effect that `effectShouldRun` finds clean. No scan
        // of the queue is needed to keep a disposed effect from running.
        scheduledEffects.delete(id);
        runEffect(id);
      }
    } finally {
      flushingEffects = false;
    }
  }

  function runEffect(id) {
    if (!effectShouldRun(id)) {
      return;
    }
    if (kinds[id] !== KIND_EFFECT) {
      return;
    }
    const node = nodes[id];
    // #lzjslazyedges: clear in place (see recomputeSlotNow) to keep the field
    // monomorphic once allocated.
    const oldDeps = node.dependencies;
    const cleanup = node.cleanup;
    node.cleanup = null;
    flags[id] &= ~F_FORCE_RUN;
    if (oldDeps !== null) {
      for (let i = 0; i < oldDeps.length; i++) {
        removeDependentEdge(oldDeps[i], id);
      }
      oldDeps.length = 0;
      // #lzspecedgeindex: see recomputeSlotNow — cleared list, stale index.
      if (dependenciesIndex.size !== 0) {
        dependenciesIndex.delete(id);
      }
    }
    if (cleanup) {
      cleanup();
    }
    // Effects track through the same fortified compute view as slots
    // (#lzcellkernel): the view is the sole tracking surface and no ambient frame
    // is pushed — the effect body reads via `view.get(...)` / `handle.get(view)`.
    const view = makeCompute(id);
    let nextCleanup;
    try {
      nextCleanup = node.run(view);
    } finally {
      view._live = false;
    }
    if (kinds[id] === KIND_EFFECT) {
      nodes[id].cleanup = typeof nextCleanup === "function" ? nextCleanup : null;
    } else if (typeof nextCleanup === "function") {
      nextCleanup();
    }
  }

  function effectShouldRun(id) {
    if (kinds[id] !== KIND_EFFECT) {
      return false;
    }
    if ((flags[id] & F_FORCE_RUN) !== 0) {
      return true;
    }
    const deps = nodes[id].dependencies;
    if (deps !== null) {
      for (let i = 0; i < deps.length; i++) {
        const dep = deps[i];
        if (kinds[dep] === KIND_SLOT && refreshSlot(dep)) {
          return true;
        }
      }
    }
    return false;
  }

  // Named so `scope()`/`withScope()` can hand the context itself to a
  // `TeardownScope` without the scope reaching into closure internals: a scope
  // is a bookkeeper over the same public surface any caller has.
  const api = {
    // #lzcellkernel primary surface (v2)
    source,
    computed,
    computedRippleWhen,
    makeEager,
    makeLazy,
    isEager,
    // deprecated constructor aliases (kept for the large internal/test surface)
    cell,
    slot,
    signal,
    effect,
    // #lzcellkernel unified read/write (v2): read both handle kinds; write source-only
    get,
    set,
    // deprecated split read/write (kept for the internal/test surface)
    getCell,
    getSignal,
    setCell,
    batch,
    disposeEffect,
    isEffectActive,
    disposeSignal,
    isSignalActive,
    disposeSlot,
    disposeCell,
    disposeNode,
    dependentCount,
    dependencyCount,
    isNodeDisposed,
    scope,
    withScope,
    isSet,
    instrumentationSnapshot,
    resetInstrumentation,
  };

  // -- Fortified compute view (#lzcellkernel) ----------------------------
  //
  // `api` doubles as the `ComputeOps` surface — the compute-time operations
  // subset (get/set/source/computed/computedRippleWhen/slot/effect/batch/dispose…)
  // implemented by the owning `Context`. The per-recompute `Compute` view below is
  // the OTHER implementor: it inherits every op from `api` (via `__proto__`) but
  // overrides `get` to be the sole *fortified* tracking surface. It carries the
  // recomputing node id AS A VALUE (`_slotId`), so a tracked read attributes to
  // that node by construction — never via an ambient carrier — which is the only
  // mechanism that stays correct across suspension in a browser (no
  // `AsyncLocalStorage`; the id is captured in the view, so a read after `await`
  // still names the right node).
  //
  // Fortification:
  //   - Sole tracking surface: `compute.get(h)` registers an edge; the explicit
  //     escape `compute.untracked().get(h)` registers none.
  //   - Non-escapable: the view is invalidated when its recompute returns
  //     (`_live = false`), so it cannot be stored and later replayed to register
  //     an edge against the wrong node — a read on a stale view throws.
  //
  // The `Compute` view is the LITERAL SOLE tracking surface (#lzcellkernel): a
  // read through it (`compute.get(...)` → `readTrackedId`) registers by
  // construction, and there is NO ambient carrier — the former `trackingStack`
  // is deleted. Every compute/effect closure (the internal eager pullers
  // `signal`/`makeEager`, relay / sem-tree / state-machine / reactive-family /
  // instrumentation, and every consumer) reads through the view it receives:
  // `compute.get(h)` or the ergonomic `handle.get(compute)`. A bare
  // `handle.get()` / top-level `ctx.get(h)` forms no edge. This matches the
  // sibling static bindings (lazily-cpp / lazily-go / lazily-dart). The
  // thread-safe / async contexts keep their own separate scoping paths.
  const untrackedView = {
    __proto__: api,
    get(handle) {
      return readTracked(handle, 0);
    },
    getCell(handle) {
      return readTracked(handle, 0);
    },
  };
  const computeProto = {
    __proto__: api,
    get(handle) {
      if (this._live === false) {
        throw new Error(
          "lazily: Compute view read after its recompute (fortification #lzcellkernel)",
        );
      }
      return readTracked(handle, this._slotId);
    },
    untracked() {
      return untrackedView;
    },
  };
  function makeCompute(slotId) {
    return { __proto__: computeProto, _slotId: slotId, _live: true };
  }

  return api;
}

// `TeardownScope` lives in its own module, re-exported here so the public API is
// unchanged. It is not merely organisational: the class carries a computed
// member key (`[Symbol.dispose]`), and a computed class key makes the class
// definition impure as far as esbuild is concerned, so the class cannot be
// tree-shaken out of a bundle that merely imports *something* from its module.
// Keeping it here cost 185 B in every consumer of `./reactive.js` — including
// `state-machine.js`, which imports one handle class and will never open a
// scope. In its own module, `"sideEffects": false` drops the whole file for
// those consumers instead.
export { TeardownScope };

/**
 * Backwards-compatible newable alias of {@link createContext}. Existing code
 * that writes `new Context(opts)` keeps working unchanged; new code may call
 * `createContext(opts)` directly. Both produce the same reactive context.
 */
export { createContext, createContext as Context };
