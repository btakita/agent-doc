// Thread-safe reactive context (lock-backed) — the `Send + Sync` flavor of
// `./reactive.js`'s `Context`, ported to isomorphic JavaScript.
//
// JavaScript is single-threaded PER REALM but shares memory across Web Workers
// (Node `worker_threads` / browser `Worker`) via `SharedArrayBuffer` + `Atomics`.
// This `ThreadSafeContext` wraps a single-threaded `Context` and serializes every
// public operation through a real Atomics mutex whose lock word lives in a
// `SharedArrayBuffer`. Share the buffer (`ctx.lockBuffer`) with a worker and
// rebind there via `ThreadSafeContext.withLockBuffer(buf, ctx)` and the two
// realms' critical sections are mutually exclusive — the JS analog of Rust's
// `Arc<RwLock<..>>`-backed `ThreadSafeContext`.
//
// The mutex is REENTRANT within a realm: run-to-completion means a synchronous
// critical section never yields, so cross-realm exclusion is enforced by the SAB
// atomic while a plain in-realm depth counter admits the re-entrant calls a
// compute/effect closure makes back into the context. When `SharedArrayBuffer` is
// unavailable (e.g. a browser without cross-origin isolation) the lock degrades
// to a no-op guard — safe, because without shared memory there is no cross-realm
// concurrency and single-realm run-to-completion already serializes access.
//
// Observationally this is IDENTICAL to the single-threaded `Context` regardless
// of interleaving — the refinement lazily-formal certifies as
// `flushBatch_singleton_eq_setSource` (thread-safe batch refines `set`) plus the
// materialization-confluence theorems consumed by `ThreadSafeReactiveMap`.
//
// Rust reference: `lazily-rs/src/thread_safe.rs`.

import { Context } from "./reactive.js";

const HAS_SHARED_MEMORY = typeof SharedArrayBuffer !== "undefined" && typeof Atomics !== "undefined";

/**
 * A reentrant mutex whose lock word optionally lives in a `SharedArrayBuffer`,
 * giving cross-worker mutual exclusion. Reentrancy within a realm is tracked by a
 * plain depth counter (only one realm can hold the SAB lock at a time, so the
 * counter is realm-private and race-free).
 */
export class AtomicMutex {
  /** @type {Int32Array | null} */
  #lock;
  /** @type {SharedArrayBuffer | ArrayBuffer | null} */
  #buffer;
  #depth = 0;

  /**
   * @param {SharedArrayBuffer} [buffer] an existing lock buffer to attach to;
   *   omit to allocate a fresh one.
   */
  constructor(buffer) {
    if (buffer) {
      this.#buffer = buffer;
      this.#lock = new Int32Array(buffer);
    } else if (HAS_SHARED_MEMORY) {
      this.#buffer = new SharedArrayBuffer(4);
      this.#lock = new Int32Array(this.#buffer);
    } else {
      this.#buffer = null;
      this.#lock = null; // degenerate single-realm guard
    }
  }

  /** The shared lock buffer to hand to a worker (null when shared memory is off). */
  get buffer() {
    return this.#buffer;
  }

  #acquire() {
    if (this.#depth > 0) {
      this.#depth++; // reentrant: this realm already holds the lock
      return;
    }
    if (this.#lock) {
      // Win the 0 -> 1 transition. Uncontended within a realm; on cross-realm
      // contention, park via Atomics.wait where allowed (workers), else spin.
      while (Atomics.compareExchange(this.#lock, 0, 0, 1) !== 0) {
        try {
          Atomics.wait(this.#lock, 0, 1);
        } catch {
          // Atomics.wait is disallowed on a main thread; busy-spin instead.
          // (Real contention only arises across workers, which can wait.)
        }
      }
    }
    this.#depth = 1;
  }

  #release() {
    this.#depth--;
    if (this.#depth === 0 && this.#lock) {
      Atomics.store(this.#lock, 0, 0);
      Atomics.notify(this.#lock, 0, 1);
    }
  }

  /**
   * Run `fn` while holding the lock; releases even if `fn` throws. Reentrant.
   * @template R
   * @param {() => R} fn
   * @returns {R}
   */
  runExclusive(fn) {
    this.#acquire();
    try {
      return fn();
    } finally {
      this.#release();
    }
  }
}

/**
 * The `Send + Sync` flavor of {@link Context}: every public operation runs under a
 * shared Atomics mutex. Handles returned here are ordinary `./reactive.js`
 * handles (the wrapped inner `Context` owns the graph). Compute/effect closures
 * capture this context and take no arguments, matching the single-threaded
 * `Context` convention.
 */
export class ThreadSafeContext {
  /** @type {Context} */
  #ctx;
  /** @type {AtomicMutex} */
  #mutex;

  /**
   * @param {{ mutex?: AtomicMutex, context?: Context, instrument?: boolean }} [opts]
   */
  constructor(opts = {}) {
    this.#ctx = opts.context ?? new Context({ instrument: opts.instrument === true });
    this.#mutex = opts.mutex ?? new AtomicMutex();
  }

  /**
   * Rebind a `ThreadSafeContext` in another realm (worker) onto a shared lock
   * buffer obtained from {@link ThreadSafeContext#lockBuffer}. NOTE: the reactive
   * graph itself does not cross the worker boundary — this shares the LOCK so a
   * cross-realm command/RPC facade driving a per-realm graph is correctly
   * serialized.
   * @param {SharedArrayBuffer} buffer
   * @param {Context} [context]
   * @returns {ThreadSafeContext}
   */
  static withLockBuffer(buffer, context) {
    return new ThreadSafeContext({ mutex: new AtomicMutex(buffer), context });
  }

  /** The shared lock buffer to hand to a worker (null when shared memory is off). */
  get lockBuffer() {
    return this.#mutex.buffer;
  }

  /** The mutex guarding this context (for {@link ThreadSafeReactiveMap}). */
  get mutex() {
    return this.#mutex;
  }

  // -- Creation ----------------------------------------------------------

  // #lzcellkernel v2 constructor surface: `source` / `computed`.
  source(value, policy) {
    return this.#mutex.runExclusive(() => this.#ctx.source(value, policy));
  }

  /** @deprecated use {@link ThreadSafeContext#source}. */
  cell(value) {
    return this.#mutex.runExclusive(() => this.#ctx.source(value));
  }

  computed(compute) {
    return this.#mutex.runExclusive(() => this.#ctx.computed(compute));
  }

  /** @deprecated use {@link ThreadSafeContext#computed} (guarded, the only derived construction). */
  slot(compute) {
    return this.#mutex.runExclusive(() => this.#ctx.computed(compute));
  }

  signal(compute) {
    return this.#mutex.runExclusive(() => this.#ctx.signal(compute));
  }

  effect(run) {
    return this.#mutex.runExclusive(() => this.#ctx.effect(run));
  }

  // -- Reads -------------------------------------------------------------

  // #lzcellkernel unified read: reads both source and computed handles.
  get(handle) {
    return this.#mutex.runExclusive(() => this.#ctx.get(handle));
  }

  /** @deprecated use {@link ThreadSafeContext#get} — the unified cell read (#lzcellkernel). */
  getCell(handle) {
    return this.#mutex.runExclusive(() => this.#ctx.get(handle));
  }

  getSignal(handle) {
    return this.#mutex.runExclusive(() => this.#ctx.getSignal(handle));
  }

  isSet(handle) {
    return this.#mutex.runExclusive(() => this.#ctx.isSet(handle));
  }

  // -- Writes / batch ----------------------------------------------------

  // #lzcellkernel unified write: only a source cell is writable (write protection).
  set(handle, value) {
    return this.#mutex.runExclusive(() => this.#ctx.set(handle, value));
  }

  /** @deprecated use {@link ThreadSafeContext#set} — the unified cell write (#lzcellkernel). */
  setCell(handle, value) {
    return this.#mutex.runExclusive(() => this.#ctx.set(handle, value));
  }

  batch(run) {
    // The whole batch (including the invalidation flush) is one critical section.
    return this.#mutex.runExclusive(() => this.#ctx.batch(run));
  }

  // -- Lifecycle ---------------------------------------------------------

  disposeEffect(handle) {
    return this.#mutex.runExclusive(() => this.#ctx.disposeEffect(handle));
  }

  isEffectActive(handle) {
    return this.#mutex.runExclusive(() => this.#ctx.isEffectActive(handle));
  }

  disposeSignal(handle) {
    return this.#mutex.runExclusive(() => this.#ctx.disposeSignal(handle));
  }

  isSignalActive(handle) {
    return this.#mutex.runExclusive(() => this.#ctx.isSignalActive(handle));
  }

  // -- Teardown + degree introspection (`#lzspecedgeindex`) --------------
  //
  // Forwarded like every other operation: a teardown mutates two edge lists per
  // node and frees an id, so it is exactly the kind of multi-step mutation the
  // lock exists to serialize. The degree reads are locked too — an unsynchronized
  // count taken mid-teardown could observe a node detached from one direction and
  // not yet the other, which is precisely the dangling half-edge these counts are
  // there to rule out.

  disposeSlot(handle) {
    return this.#mutex.runExclusive(() => this.#ctx.disposeSlot(handle));
  }

  disposeCell(handle) {
    return this.#mutex.runExclusive(() => this.#ctx.disposeCell(handle));
  }

  disposeNode(handle) {
    return this.#mutex.runExclusive(() => this.#ctx.disposeNode(handle));
  }

  dependentCount(handle) {
    return this.#mutex.runExclusive(() => this.#ctx.dependentCount(handle));
  }

  dependencyCount(handle) {
    return this.#mutex.runExclusive(() => this.#ctx.dependencyCount(handle));
  }

  isNodeDisposed(handle) {
    return this.#mutex.runExclusive(() => this.#ctx.isNodeDisposed(handle));
  }

  /**
   * Open a teardown scope over this context. Every scope operation — including
   * the whole reverse-order teardown in `end()` — runs as ONE critical section,
   * so another realm never observes a half-torn-down scope.
   */
  scope() {
    return new ThreadSafeTeardownScope(this.#ctx.scope(), this.#mutex);
  }

  /** Run `body` with a fresh scope and end it in a `finally`. */
  withScope(body) {
    const s = this.scope();
    try {
      return body(s);
    } finally {
      s.end();
    }
  }

  // -- Instrumentation (opt-in; passthrough) -----------------------------

  instrumentationSnapshot() {
    return this.#mutex.runExclusive(() =>
      typeof this.#ctx.instrumentationSnapshot === "function"
        ? this.#ctx.instrumentationSnapshot()
        : null,
    );
  }

  resetInstrumentation() {
    return this.#mutex.runExclusive(() => {
      if (typeof this.#ctx.resetInstrumentation === "function") {
        this.#ctx.resetInstrumentation();
      }
    });
  }
}

// See the note on `DISPOSE` in ./reactive.js.
const DISPOSE = Symbol.dispose ?? Symbol.for("Symbol.dispose");

/**
 * A {@link TeardownScope} whose every operation runs under the context's mutex.
 *
 * Wrapping rather than re-implementing keeps one definition of scope semantics —
 * reverse-order teardown, idempotent `end`, `disarm` that touches no node — and
 * lets this class carry only the concern it actually owns: the critical section.
 * `end()` takes the lock ONCE for the whole reverse walk, not once per node, so
 * a scope tears down atomically from another realm's point of view.
 */
export class ThreadSafeTeardownScope {
  #inner;
  #mutex;

  /** @internal — obtain one from {@link ThreadSafeContext#scope}. */
  constructor(inner, mutex) {
    this.#inner = inner;
    this.#mutex = mutex;
  }

  get size() {
    return this.#mutex.runExclusive(() => this.#inner.size);
  }

  get ended() {
    return this.#mutex.runExclusive(() => this.#inner.ended);
  }

  adopt(handle) {
    return this.#mutex.runExclusive(() => this.#inner.adopt(handle));
  }

  source(value, policy) {
    return this.#mutex.runExclusive(() => this.#inner.source(value, policy));
  }

  /** @deprecated use {@link ThreadSafeTeardownScope#source}. */
  cell(value) {
    return this.#mutex.runExclusive(() => this.#inner.source(value));
  }

  computed(compute) {
    return this.#mutex.runExclusive(() => this.#inner.computed(compute));
  }

  signal(compute) {
    return this.#mutex.runExclusive(() => this.#inner.signal(compute));
  }

  effect(run) {
    return this.#mutex.runExclusive(() => this.#inner.effect(run));
  }

  disarm() {
    return this.#mutex.runExclusive(() => this.#inner.disarm());
  }

  end() {
    return this.#mutex.runExclusive(() => this.#inner.end());
  }

  /** TC39 explicit resource management: `using scope = ctx.scope()`. */
  [DISPOSE]() {
    this.end();
  }
}
