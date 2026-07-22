export type ComputeFn<T> = (cx: Compute) => T;
export type EffectRun = (cx: Compute) => (() => void) | null | undefined;
/**
 * The compute-time READ surface (#lzcellkernel): the subset of {@link Context}
 * that registers dependency tracking. Both the owning `Context` (an untracked
 * read at top level) and the per-recompute fortified `Compute` view (a tracked
 * read inside a compute/effect closure) satisfy it, so a reactive-read method
 * takes a `ComputeOps` and callers thread the compute view they received to
 * subscribe. This is the value-threaded replacement for ambient tracking.
 */
export interface ComputeOps {
  get<T>(handle: Source<T> | Computed<T>): T;
}
/**
 * The per-recompute fortified compute view handed to a compute/effect closure
 * (#lzcellkernel). It is the LITERAL sole tracking surface — there is no ambient
 * carrier — so a read through it (`cx.get(h)` or `handle.get(cx)`) registers a
 * dependency edge against the recomputing node, and {@link Compute.untracked}
 * yields a sibling surface whose reads form none. The view is retired the moment
 * its recompute returns; a read on a stale view throws.
 */
export interface Compute extends ComputeOps {
  /** The untracked escape: reads through the returned surface form no edge. */
  untracked(): ComputeOps;
}
export type EqualFn<T> = (a: T, b: T) => boolean;
/**
 * Custom propagate predicate for {@link Context.computedRippleWhen}: return
 * `true` to PROPAGATE the recompute downstream, `false` to SUPPRESS it. MUST be
 * pure in `(old, new)` — value-carried state (version/counter) is fine; external
 * mutable state is not.
 */
export type ChangedFn<T> = (old: T, next: T) => boolean;

/**
 * Default structural equality used by cells and memos: identity, then
 * byte-wise `Uint8Array`, then `Array` index recursion, then a key-wise
 * shallow/deep object comparison. Exposed so consumers can reuse the canonical
 * equality in custom {@link EqualFn} composition and benchmarks.
 */
export declare function defaultEqual<T>(a: T, b: T): boolean;

/**
 * Thrown when a node that has been torn down is read or written
 * (`read_after_dispose`). Narrowable with `instanceof`, so a caller — an effect
 * body, a teardown path — can act on this cause specifically instead of
 * swallowing every error.
 */
export class DisposedNodeError extends Error {
  readonly name: "DisposedNodeError";
  readonly nodeId: number;
}

/** Any node handle a {@link Context} can address by degree or tear down. */
export type NodeHandle =
  | Source<never>
  | Computed<never>
  | Effect
  | SignalHandle<never>;

/**
 * A cell written from outside, folding writes under an optional merge policy
 * (default keep-latest = replace). Subsumes the former `MergeCell` wrapper: a
 * keep-latest `Source` is the plain cell. The writable value handle (v1
 * `SourceCell`).
 *
 * The read/write split has no compile-time enforcement in JavaScript; it is
 * expressed by METHOD PRESENCE — a `Source` exposes `set`/`merge`, a `Computed`
 * does not.
 */
export class Source<T = unknown> {
  /** @internal */ constructor(id: number, ctx?: unknown);
  readonly id: number;
  /**
   * Read the current value. Thread the {@link Compute} view (`cx`) a
   * compute/effect closure received to register a dependency edge; a bare
   * `get()` at top level (or inside `untracked()`) forms no edge.
   */
  get(cx?: ComputeOps): T;
  /** Replace the value outright (the keep-latest write). */
  set(value: T): void;
  /** Fold `op` into the current value under this cell's policy (replace if none). */
  merge(op: T): void;
  /** Tear this node down. */
  dispose(): void;
}

/**
 * A cell computed from upstream — always guarded (an equal recompute suppresses
 * downstream; matches TC39 `Signal.Computed`). Lazy by default;
 * `computed(f).eager()` makes it eager. Exposes no `set`/`merge` (v1
 * `FormulaCell`).
 */
export class Computed<T = unknown> {
  /** @internal */ constructor(id: number, ctx?: unknown);
  readonly id: number;
  /**
   * Read the current value. Thread the {@link Compute} view (`cx`) a
   * compute/effect closure received to register a dependency edge; a bare
   * `get()` at top level (or inside `untracked()`) forms no edge.
   */
  get(cx?: ComputeOps): T;
  /** Make this computed eager (attach a puller). Idempotent; returns `this`. */
  eager(): this;
  /** Reverse of {@link eager}: revert to lazy and dispose the puller. */
  lazy(): this;
  /** Whether this computed is currently eager (has an active puller). */
  isEager(): boolean;
  /** Tear this node down. */
  dispose(): void;
}

/** A value-less side-effecting sink. Outside the Cell hierarchy (v1 `EffectHandle`). */
export class Effect {
  /** @internal */ constructor(id: number, ctx?: unknown);
  readonly id: number;
  /** Dispose this effect (unsubscribe). */
  dispose(): void;
}

/**
 * @deprecated v1 type aliases retained so peripheral modules' declaration files
 * still resolve during the staged family-wide rename. The canonical handles are
 * {@link Source} / {@link Computed} / {@link Effect}. No runtime alias exists.
 */
export type CellHandle<T = unknown> = Source<T>;
/** @deprecated use {@link Computed}. */
export type SlotHandle<T = unknown> = Computed<T>;
/** @deprecated use {@link Effect}. */
export type EffectHandle = Effect;

/**
 * @deprecated Retired by the Cell kernel (#lzcellkernel): the eager construction
 * is `ctx.computed(f).eager()`. Retained as a compatibility shape for the
 * thread-safe / async contexts and `state-machine`.
 */
export class SignalHandle<T = unknown> {
  /** @internal */ constructor(slot: Computed<T>, effect: Effect);
  readonly slot: Computed<T>;
  readonly effect: Effect;
}

export interface InstrumentationSnapshot {
  nodeAllocations: number;
  slotRecomputes: number;
  dependencyEdgesAdded: number;
  dependencyEdgesRemoved: number;
  effectQueuePushes: number;
  maxEffectQueueDepth: number;
}

/**
 * A reactive context: the graph owning all Source/Computed/Effect nodes
 * (the Cell kernel, `#lzcellkernel`).
 *
 * Declared as an interface so it can be used as a type
 * (`import type { Context }`) while the runtime value is the newable
 * {@link Context} function below (declaration merging: same name, a type side
 * and a value side). New code may prefer {@link createContext}.
 */
/** A merge policy: an associative fold `merge(old, op) -> T` plus law flags. */
export interface MergePolicy<T> {
  merge(old: T, op: T): T;
  name?: string;
  commutative?: boolean;
  idempotent?: boolean;
  conflates?: boolean;
}

export interface Context {
  // -- Cell kernel (#lzcellkernel) primary surface (v2) --
  /** A source cell (keep-latest); `source(v, policy)` folds under `policy`. */
  source<T>(value: T, policy?: MergePolicy<T>): Source<T>;
  /** A guarded computed (equality-suppressed, always) — the derived construction. */
  computed<T>(compute: ComputeFn<T>): Computed<T>;
  /**
   * A guarded computed whose downstream propagation is gated by an explicit,
   * PURE predicate `changed(old, new)` (`true` = propagate, `false` = suppress)
   * instead of the natural {@link defaultEqual} (#lzcellkernel). Always computes
   * (propagate guard, not compute guard). `computed(f)` ≡
   * `computedRippleWhen(f, (o, n) => !defaultEqual(o, n))`; a pass-through
   * (always propagate) is `computedRippleWhen(f, () => true)`.
   */
  computedRippleWhen<T>(compute: ComputeFn<T>, changed: ChangedFn<T>): Computed<T>;
  /** Make a computed eager by id (prefer {@link Computed.eager}). Idempotent. */
  makeEager(id: number): void;
  /** Reverse of {@link makeEager}. */
  makeLazy(id: number): void;
  /** Whether the computed is eager. */
  isEager(handle: Computed<unknown> | number): boolean;

  // -- Deprecated constructor aliases --
  /** @deprecated use {@link source}. */
  cell<T>(value: T): Source<T>;
  /** @deprecated use {@link computed} (guarded, the only derived construction). */
  slot<T>(compute: ComputeFn<T>): Computed<T>;
  /** @deprecated the eager construction is `computed(f).eager()`. */
  signal<T>(compute: ComputeFn<T>): SignalHandle<T>;
  effect(run: EffectRun): Effect;
  // -- Cell kernel (#lzcellkernel) unified read/write (v2) --
  /** The unified cell read: a `Source` returns its value, a `Computed` recomputes. */
  get<T>(handle: Source<T> | Computed<T>): T;
  /** The unified cell write: only a `Source` is writable (write protection). */
  set<T>(handle: Source<T>, value: T): void;
  // -- Deprecated split read/write --
  /** @deprecated use {@link get} — the unified cell read (#lzcellkernel). */
  getCell<T>(handle: Source<T>): T;
  getSignal<T>(handle: SignalHandle<T>): T;
  /** @deprecated use {@link set} — the unified cell write (#lzcellkernel). */
  setCell<T>(handle: Source<T>, value: T): void;
  batch(run: () => void): void;
  disposeEffect(handle: Effect): void;
  isEffectActive(handle: Effect): boolean;
  disposeSignal(handle: SignalHandle<unknown>): void;
  isSignalActive(handle: SignalHandle<unknown>): boolean;
  /**
   * Tear down a lazy computed: detach its upstream and downstream dependency
   * edges, free the node, and recycle its id. No-op on an already-disposed handle
   * or the wrong kind. Callers must ensure no live compute still reads it.
   */
  disposeSlot<T>(handle: Computed<T>): void;
  /**
   * Tear down a source cell: detach its downstream edges, free the node, and
   * recycle its id. No-op on an already-disposed handle or the wrong kind. Callers
   * must ensure no live computed still reads the cell (its next recompute would throw).
   */
  disposeCell<T>(handle: Source<T>): void;
  /**
   * Tear down whatever kind of node `handle` names. Dispatch is on the handle's
   * CLASS, not on the node currently occupying its id: ids are recycled, and a
   * stale handle whose id has been reissued must be a no-op rather than tear
   * down the new occupant.
   */
  disposeNode(handle: NodeHandle): void;
  /**
   * How many nodes currently depend on `handle` — the size of its reverse edge
   * set. 0 for a disposed node and for effects, which are pure sinks.
   *
   * Counts, never collections: graph shape is assertable with no path to the
   * edge arrays and no storage strategy pinned into the contract.
   */
  dependentCount(handle: NodeHandle): number;
  /**
   * How many nodes `handle` currently depends on — the size of its forward edge
   * set. 0 for a disposed node and for cells, which are pure sources.
   */
  dependencyCount(handle: NodeHandle): number;
  /** Whether `handle`'s id currently names no live node. Ids are recycled, so
   * this means "disposed and not yet reused". */
  isNodeDisposed(handle: NodeHandle): boolean;
  /** Open a {@link TeardownScope} over this context. */
  scope(): TeardownScope;
  /** Run `body` with a fresh scope and end it in a `finally`. */
  withScope<R>(body: (scope: TeardownScope) => R): R;
  isSet<T>(handle: Computed<T>): boolean;
  /** Instrumentation counters, or `null` if not enabled at construction. */
  instrumentationSnapshot(): InstrumentationSnapshot | null;
  /** Zero the instrumentation counters (no-op when instrumentation is off). */
  resetInstrumentation(): void;
}

/**
 * Create a reactive {@link Context} — the idiomatic entry point.
 *
 * Implemented with the closure factory technique (#lzjsclosure): graph state is
 * captured in closure bindings rather than class instance fields, so V8 inlines
 * the hot paths more aggressively than the prior `class` + `#private` version.
 */
export declare function createContext(opts?: { instrument?: boolean }): Context;

/**
 * A teardown scope over a {@link Context}: nodes created through it are disposed
 * when it ends, in reverse creation order (`#lzspecedgeindex`).
 *
 * The end is explicit — `withScope`, `using` via `Symbol.dispose`, or `end()`.
 * JavaScript has no destructor, and the only GC-driven hook
 * (`FinalizationRegistry`) is not guaranteed to run, so a scope that tore down
 * on collection would reintroduce the very leak scopes exist to prevent.
 * `end()` also covers what no bracket can express: a scope whose lifetime is a
 * connection or a subscription, opened in one callback and ended in another.
 */
export class TeardownScope {
  /** @internal — obtain one from {@link Context.scope} / {@link Context.withScope}. */
  private constructor(ctx: Context);
  /** How many nodes this scope currently owns. */
  readonly size: number;
  /** Whether {@link end} has already run. */
  readonly ended: boolean;
  adopt<H extends NodeHandle>(handle: H): H;
  /** Create a source cell owned by this scope (#lzcellkernel). */
  source<T>(value: T, policy?: MergePolicy<T>): Source<T>;
  /** Create a guarded computed owned by this scope (#lzcellkernel). */
  computed<T>(compute: ComputeFn<T>): Computed<T>;
  /** Create a guarded computed with a custom propagate predicate, owned by this scope (#lzcellkernel). */
  computedRippleWhen<T>(compute: ComputeFn<T>, changed: ChangedFn<T>): Computed<T>;
  /** @deprecated use {@link source}. */
  cell<T>(value: T): Source<T>;
  /** @deprecated use {@link computed} (guarded, the only derived construction). */
  slot<T>(compute: ComputeFn<T>): Computed<T>;
  /** @deprecated the eager construction is `computed(f).eager()`. */
  signal<T>(compute: ComputeFn<T>): SignalHandle<T>;
  effect(run: EffectRun): Effect;
  /** Cancel this scope's teardown. The nodes are untouched — same values, same
   * edges, still individually disposable. */
  disarm(): void;
  /** Dispose every node this scope owns, in reverse creation order. Idempotent. */
  end(): void;
  [Symbol.dispose](): void;
}

/**
 * Backwards-compatible newable wrapper around {@link createContext}. Existing
 * `new Context(opts)` call sites keep working unchanged; new code may call
 * `createContext(opts)` directly. Both return the same reactive context.
 */
export declare function Context(opts?: { instrument?: boolean }): Context;
