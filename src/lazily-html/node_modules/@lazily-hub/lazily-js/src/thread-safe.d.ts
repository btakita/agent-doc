import type {
  CellHandle,
  Context,
  EffectHandle,
  NodeHandle,
  SignalHandle,
  SlotHandle,
} from "./reactive.js";

/**
 * A reentrant mutex whose lock word optionally lives in a `SharedArrayBuffer`,
 * giving cross-worker mutual exclusion. Degrades to a no-op guard when shared
 * memory is unavailable.
 */
export class AtomicMutex {
  constructor(buffer?: SharedArrayBuffer);
  readonly buffer: SharedArrayBuffer | ArrayBuffer | null;
  runExclusive<R>(fn: () => R): R;
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
 * The `Send + Sync` flavor of {@link Context}: every public operation runs under
 * a shared Atomics mutex. Compute/effect closures capture this context and take
 * no arguments.
 */
export class ThreadSafeContext {
  constructor(opts?: { mutex?: AtomicMutex; context?: Context; instrument?: boolean });

  /** Rebind a `ThreadSafeContext` in another realm onto a shared lock buffer. */
  static withLockBuffer(buffer: SharedArrayBuffer, context?: Context): ThreadSafeContext;

  /** The shared lock buffer to hand to a worker (null when shared memory is off). */
  readonly lockBuffer: SharedArrayBuffer | ArrayBuffer | null;
  /** The mutex guarding this context. */
  readonly mutex: AtomicMutex;

  // #lzcellkernel v2 constructor surface
  source<T>(value: T, policy?: unknown): CellHandle<T>;
  computed<T>(compute: () => T): SlotHandle<T>;
  /** @deprecated use {@link ThreadSafeContext#source}. */
  cell<T>(value: T): CellHandle<T>;
  /** @deprecated use {@link ThreadSafeContext#computed} (guarded, the only derived construction). */
  slot<T>(compute: () => T): SlotHandle<T>;
  signal<T>(compute: () => T): SignalHandle<T>;
  effect(run: () => void | (() => void)): EffectHandle;

  // #lzcellkernel unified read/write
  get<T>(handle: CellHandle<T> | SlotHandle<T>): T;
  set<T>(handle: CellHandle<T>, value: T): void;
  /** @deprecated use {@link ThreadSafeContext#get} — the unified cell read (#lzcellkernel). */
  getCell<T>(handle: CellHandle<T>): T;
  getSignal<T>(handle: SignalHandle<T>): T;
  isSet<T>(handle: SlotHandle<T>): boolean;

  /** @deprecated use {@link ThreadSafeContext#set} — the unified cell write (#lzcellkernel). */
  setCell<T>(handle: CellHandle<T>, value: T): void;
  batch(run: () => void): void;

  disposeEffect(handle: EffectHandle): void;
  isEffectActive(handle: EffectHandle): boolean;
  disposeSignal<T>(handle: SignalHandle<T>): void;
  isSignalActive<T>(handle: SignalHandle<T>): boolean;

  disposeSlot<T>(handle: SlotHandle<T>): void;
  disposeCell<T>(handle: CellHandle<T>): void;
  disposeNode(handle: NodeHandle): void;
  dependentCount(handle: NodeHandle): number;
  dependencyCount(handle: NodeHandle): number;
  isNodeDisposed(handle: NodeHandle): boolean;
  /** Open a teardown scope whose every operation — including the whole
   * reverse-order teardown in `end()` — runs as one critical section. */
  scope(): ThreadSafeTeardownScope;
  withScope<R>(body: (scope: ThreadSafeTeardownScope) => R): R;

  instrumentationSnapshot(): InstrumentationSnapshot | null;
  resetInstrumentation(): void;
}

/**
 * A teardown scope whose every operation runs under the context's mutex.
 * Wraps the single-threaded `TeardownScope`, so scope semantics have exactly one
 * definition and this class carries only the critical section.
 */
export class ThreadSafeTeardownScope {
  /** @internal — obtain one from {@link ThreadSafeContext.scope}. */
  private constructor(inner: unknown, mutex: AtomicMutex);
  readonly size: number;
  readonly ended: boolean;
  adopt<H extends NodeHandle>(handle: H): H;
  source<T>(value: T, policy?: unknown): CellHandle<T>;
  /** @deprecated use {@link ThreadSafeTeardownScope#source}. */
  cell<T>(value: T): CellHandle<T>;
  computed<T>(compute: () => T): SlotHandle<T>;
  signal<T>(compute: () => T): SignalHandle<T>;
  effect(run: () => void | (() => void)): EffectHandle;
  disarm(): void;
  end(): void;
  [Symbol.dispose](): void;
}
