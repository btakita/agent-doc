export type AsyncSlotStateView = "none" | "empty" | "computing" | "resolved" | "error";

export interface AsyncComputeContext {
  /** Aborts when this run is superseded by a newer revision or disposed. */
  readonly signal: AbortSignal;
  /** Unified cell read (#lzcellkernel), recording it as a dependency (synchronous). */
  get<T>(handle: AsyncSource<T>): T;
  /** @deprecated use {@link get} — the unified cell read (#lzcellkernel). */
  getCell<T>(handle: AsyncSource<T>): T;
  /** Await a slot value, recording it as a dependency before awaiting. */
  getAsync<T>(handle: AsyncComputed<T>): Promise<T>;
  /** Await a signal value, recording its slot as a dependency before awaiting. */
  getSignalAsync<T>(handle: AsyncSignalHandle<T>): Promise<T>;
}

export type AsyncComputeFn<T> = (ctx: AsyncComputeContext) => T | Promise<T>;
export type AsyncCleanup = (() => void) | (() => Promise<void>);
export type AsyncEffectRun = (
  ctx: AsyncComputeContext,
) => AsyncCleanup | null | undefined | Promise<AsyncCleanup | null | undefined>;

export class AsyncSource<T = unknown> {
  /** @internal */ constructor(id: number);
  readonly id: number;
}

export class AsyncComputed<T = unknown> {
  /** @internal */ constructor(id: number);
  readonly id: number;
}

/** @deprecated use {@link AsyncSource}. */
export const AsyncCellHandle: typeof AsyncSource;
/** @deprecated use {@link AsyncSource}. */
export type AsyncCellHandle<T = unknown> = AsyncSource<T>;

/** @deprecated use {@link AsyncComputed}. */
export const AsyncSlotHandle: typeof AsyncComputed;
/** @deprecated use {@link AsyncComputed}. */
export type AsyncSlotHandle<T = unknown> = AsyncComputed<T>;

export class AsyncEffectHandle {
  /** @internal */ constructor(id: number);
  readonly id: number;
}

export class AsyncSignalHandle<T = unknown> {
  /** @internal */ constructor(slot: AsyncComputed<T>, effect: AsyncEffectHandle);
  readonly slot: AsyncComputed<T>;
  readonly effect: AsyncEffectHandle;
}

export class AsyncContext {
  /** Create a source cell (the synchronous input layer) (#lzcellkernel). */
  source<T>(value: T): AsyncSource<T>;
  /** @deprecated use {@link AsyncContext#source}. */
  cell<T>(value: T): AsyncSource<T>;
  /** The unified cell write (#lzcellkernel): only a source cell is writable. */
  set<T>(handle: AsyncSource<T>, value: T): void;
  /** @deprecated use {@link AsyncContext#get} — the unified cell read (#lzcellkernel). */
  getCell<T>(handle: AsyncSource<T>): T;
  /** @deprecated use {@link AsyncContext#set} — the unified cell write (#lzcellkernel). */
  setCell<T>(handle: AsyncSource<T>, value: T): void;

  /** Create an async computed slot (no memo guard). */
  computedAsync<T>(compute: AsyncComputeFn<T>): AsyncComputed<T>;
  /** Create an async computed slot with an equality memo guard. */
  memoAsync<T>(compute: AsyncComputeFn<T>): AsyncComputed<T>;
  /** Create an eager async signal (memo slot + puller effect). */
  signalAsync<T>(compute: AsyncComputeFn<T>): AsyncSignalHandle<T>;
  /** Create an async effect returning an optional (possibly async) cleanup. */
  effectAsync(run: AsyncEffectRun): AsyncEffectHandle;

  /**
   * The unified cell read (#lzcellkernel). A source cell returns its value; a
   * slot returns its synchronous cached snapshot (resolved value or `undefined`).
   */
  get<T>(handle: AsyncSource<T>): T;
  get<T>(handle: AsyncComputed<T>): T | undefined;
  isResolved<T>(handle: AsyncComputed<T>): boolean;
  /** Public projection of the slot state machine. */
  slotState<T>(handle: AsyncComputed<T>): AsyncSlotStateView;
  /** Await a slot value (fast path via `get()`, else spawn/attach). */
  getAsync<T>(handle: AsyncComputed<T>): Promise<T>;
  getSignal<T>(handle: AsyncSignalHandle<T>): T | undefined;
  getSignalAsync<T>(handle: AsyncSignalHandle<T>): Promise<T>;

  /** Synchronous batch boundary; async reruns are scheduled at batch exit. */
  batch(run: () => void): void;
  /** Await scheduled reruns and in-flight computes until the graph quiesces. */
  settle(): Promise<void>;

  /** Dispose an async effect and await its cleanup. */
  disposeAsyncEffect(handle: AsyncEffectHandle): Promise<void>;
  isEffectActive(handle: AsyncEffectHandle): boolean;
  disposeSignal(handle: AsyncSignalHandle<unknown>): Promise<void>;
  isSignalActive(handle: AsyncSignalHandle<unknown>): boolean;
  /** Dispose the context: abort in-flight work and await active cleanups. */
  dispose(): Promise<void>;

  /** Tear down an async derived slot: mark the surviving cone stale, supersede
   * any in-flight compute, then detach both edge directions. Idempotent. */
  disposeSlot<T>(handle: AsyncComputed<T>): void;
  /** Tear down a source cell. See {@link disposeSlot}. */
  disposeCell<T>(handle: AsyncSource<T>): void;
  /** Tear down whatever kind of node `handle` names. Dispatch is on the
   * handle's CLASS, not on the node currently at its id — ids are recycled, and
   * a stale handle must be a no-op rather than tear down the new occupant. */
  disposeNode(handle: AsyncNodeHandle): Promise<void>;
  /** How many nodes currently depend on `handle`. Counts, never collections. */
  dependentCount(handle: AsyncNodeHandle): number;
  /** How many nodes `handle` currently depends on. */
  dependencyCount(handle: AsyncNodeHandle): number;
  /** Whether `handle`'s id currently names no live node. */
  isNodeDisposed(handle: AsyncNodeHandle): boolean;
  /** Open an {@link AsyncTeardownScope} over this context. */
  scope(): AsyncTeardownScope;
  /** Run `body` with a fresh scope and end it in a `finally`. */
  withScope<R>(body: (scope: AsyncTeardownScope) => R | Promise<R>): Promise<R>;
}

/** Any node handle an {@link AsyncContext} can address by degree or tear down. */
export type AsyncNodeHandle =
| AsyncSource<never>
| AsyncComputed<never>
  | AsyncEffectHandle
  | AsyncSignalHandle<never>;

/**
 * A teardown scope over an {@link AsyncContext}: nodes created through it are
 * disposed when it ends, in reverse creation order (`#lzspecedgeindex`).
 *
 * `end()` is a promise because async effect teardown awaits the effect's run
 * loop and its cleanup, and each node is awaited before the next is torn down —
 * which is what keeps cleanup order observable and deterministic.
 */
export class AsyncTeardownScope {
  /** @internal — obtain one from {@link AsyncContext.scope}. */
  private constructor(ctx: AsyncContext);
  readonly size: number;
  readonly ended: boolean;
  adopt<H extends AsyncNodeHandle>(handle: H): H;
  source<T>(value: T): AsyncSource<T>;
  /** @deprecated use {@link AsyncTeardownScope#source}. */
  cell<T>(value: T): AsyncSource<T>;
  computedAsync<T>(compute: AsyncComputeFn<T>): AsyncComputed<T>;
  memoAsync<T>(compute: AsyncComputeFn<T>): AsyncComputed<T>;
  signalAsync<T>(compute: AsyncComputeFn<T>): AsyncSignalHandle<T>;
  effectAsync(run: AsyncEffectRun): AsyncEffectHandle;
  /** Cancel this scope's teardown; the nodes themselves are untouched. */
  disarm(): void;
  /** Dispose every node this scope owns, in reverse creation order. Idempotent. */
  end(): Promise<void>;
  [Symbol.asyncDispose](): Promise<void>;
}

export { DisposedNodeError } from "./reactive.js";
