// Async reactive graph (lazily-spec/docs/async.md) — the native JavaScript
// counterpart of lazily-rs's `AsyncContext` and lazily-kt's `AsyncContext`.
//
// This is a SEPARATE reactive surface from `./reactive`, not an overload: async
// derivations introduce in-flight state, revision-guarded stale-completion
// discard, in-flight deduplication, cancellation, and dependency tracking across
// `await` suspension points that the synchronous graph does not have. Cells are
// the synchronous input layer; only computed slots, memos, and effects are async.
//
// Because JavaScript is single-threaded and cooperatively scheduled, this port
// drops the lazily-rs locking and id-generation guards: a synchronous section
// between two `await`s is atomic, so revision-check-then-publish needs no lock,
// and GC'd handle objects mean an in-flight run can never write into a recycled
// slot. What carries over is the *observable* contract: the Empty/Computing/
// Resolved/Error state machine, revision-guarded publish, one in-flight compute
// per revision shared by all waiters, cooperative abort on supersession/dispose,
// and cleanup-before-rerun ordering for effects.

// `DisposedNodeError` is imported rather than redeclared: the runner — and any
// caller with both graphs in play — narrows read-after-dispose with a single
// `instanceof`, which a per-module copy of the class would silently break.
import { DisposedNodeError } from "./reactive.js";

export { DisposedNodeError };

function defaultEqual(a, b) {
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
    if (
      !Object.prototype.hasOwnProperty.call(b, k) ||
      !defaultEqual(a[k], b[k])
    ) {
      return false;
    }
  }
  return true;
}

// -- Handles -----------------------------------------------------------------

export class AsyncSource {
  /** @internal */ constructor(id) {
    this.id = id;
    Object.freeze(this);
  }
}

export class AsyncComputed {
  /** @internal */ constructor(id) {
    this.id = id;
    Object.freeze(this);
  }
}

/** @deprecated use {@link AsyncSource}. */
export const AsyncCellHandle = AsyncSource;

/** @deprecated use {@link AsyncComputed}. */
export const AsyncSlotHandle = AsyncComputed;

export class AsyncEffectHandle {
  /** @internal */ constructor(id) {
    this.id = id;
    Object.freeze(this);
  }
}

export class AsyncSignalHandle {
  /** @internal */ constructor(slot, effect) {
    this.slot = slot;
    this.effect = effect;
    Object.freeze(this);
  }
}

// -- Completion notifier (one per in-flight compute) -------------------------
//
// Every waiter attached to an in-flight compute awaits the same notifier. The
// compute settles it exactly once with a resolved value, an error, or a
// `superseded` signal ("the world changed — re-resolve from current state").

function makeNotifier() {
  let resolveFn;
  const promise = new Promise((resolve) => {
    resolveFn = resolve;
  });
  return {
    promise,
    done: false,
    settle(event) {
      if (!this.done) {
        this.done = true;
        resolveFn(event);
      }
    },
  };
}

// -- Nodes -------------------------------------------------------------------

// #lzjsasyncarrays: per-node dependency/dependent sets replaced by dedup'd
// arrays (mirrors the sync `reactive.js` core). A Set costs meaningfully more
// per instance than a small array; async graphs rarely have wide fan-out per
// node, so the array + linear dedup wins on allocation.
function edgeInsert(edges, id) {
  for (let i = 0; i < edges.length; i++) {
    if (edges[i] === id) {
      return false;
    }
  }
  edges.push(id);
  return true;
}

function edgeRemove(edges, id) {
  for (let i = 0; i < edges.length; i++) {
    if (edges[i] === id) {
      edges[i] = edges[edges.length - 1];
      edges.pop();
      return true;
    }
  }
  return false;
}

class AsyncCellNode {
  constructor(value) {
    this.value = value;
    this.dependents = [];
  }
}

class AsyncSlotNode {
  constructor(compute, memo) {
    this.compute = compute;
    this.memo = memo; // equality-guarded recompute
    this.state = "empty"; // empty | computing | resolved | error
    this.value = undefined;
    this.hasValue = false;
    this.error = undefined;
    this.revision = 0;
    this.token = null; // current in-flight token { revision, aborted }
    this.abort = null; // AbortController for the in-flight compute
    this.notifier = null; // completion notifier for the in-flight compute
    this.dependencies = [];
    this.dependents = [];
  }
}

class AsyncEffectNode {
  constructor(run) {
    this.run = run;
    this.dependencies = [];
    this.cleanup = null; // cleanup fn from the last completed body
    this.abort = null; // AbortController for the in-flight body
    this.pending = false; // a rerun is queued
    this.kicking = false; // the serialized run loop is active
  }
}

// -- Context -----------------------------------------------------------------

export class AsyncContext {
  // #lzjsasyncarrays: nodes backed by a sparse array + recycled id stack
  // (mirrors the sync core) instead of a Map. Array index lookup is faster than
  // Map.get and avoids per-entry Map bucket allocation.
  #nodes = [];
  #nextId = 1;
  #freeIds = [];
  #inflight = new Map(); // slotId -> Promise of the current compute task
  #effectRuns = new Set(); // in-flight effect run-loop promises
  #batchDepth = 0;
  #batchedCells = new Set();
  // #lzspecedgeindex: depth of the disposal-driven invalidation cascade. While
  // it is non-zero the cascade is MARK-ONLY — `#scheduleEffect` drops every
  // effect it reaches, because disposal is not a publish. See the identical
  // counter in `./reactive.js` for the full reasoning. A counter rather than a
  // flag so a scope tearing down N nodes nests correctly.
  #disposalDepth = 0;

  // -- Creation ----------------------------------------------------------

  // #lzcellkernel v2 constructor: `source` is the canonical source-cell name.
  source(value) {
    const id = this.#allocId();
    this.#nodes[id] = new AsyncCellNode(value);
    return new AsyncSource(id);
  }

  /** @deprecated use {@link AsyncContext#source}. */
  cell(value) {
    return this.source(value);
  }

  computedAsync(compute) {
    const id = this.#allocId();
    this.#nodes[id] = new AsyncSlotNode(compute, false);
    return new AsyncComputed(id);
  }

  memoAsync(compute) {
    const id = this.#allocId();
    this.#nodes[id] = new AsyncSlotNode(compute, true);
    return new AsyncComputed(id);
  }

  effectAsync(run) {
    const id = this.#allocId();
    this.#nodes[id] = new AsyncEffectNode(run);
    this.#scheduleEffect(id);
    return new AsyncEffectHandle(id);
  }

  signalAsync(compute) {
    const slotId = this.#allocId();
    this.#nodes[slotId] = new AsyncSlotNode(compute, true);
    const slot = new AsyncComputed(slotId);
    const effectId = this.#allocId();
    this.#nodes[effectId] = new AsyncEffectNode(async (cctx) => {
      await cctx.getAsync(slot);
      return null;
    });
    this.#scheduleEffect(effectId);
    return new AsyncSignalHandle(slot, new AsyncEffectHandle(effectId));
  }

  // -- Cells (synchronous input layer) -----------------------------------

  /**
   * The unified cell read of the Cell kernel (#lzcellkernel). A source returns
   * its stored value; a computed returns its resolved snapshot or `undefined`.
   */
  get(handle) {
    const node = this.#nodes[handle.id];
    if (node === undefined) {
      throw new DisposedNodeError(handle.id);
    }
    if (node instanceof AsyncCellNode) {
      return node.value;
    }
    if (node instanceof AsyncSlotNode && node.state === "resolved") {
      return node.value;
    }
    return undefined;
  }

  /** @deprecated use {@link AsyncContext#get} — the unified cell read (#lzcellkernel). */
  getCell(handle) {
    return this.get(handle);
  }

  /**
   * The unified cell write of the Cell kernel (#lzcellkernel). Only a source
   * cell is writable (write protection); a computed/slot handle throws.
   */
  set(handle, value) {
    const node = this.#nodes[handle.id];
    // A write that silently vanishes is the same failure mode as a read that
    // silently returns stale.
    if (node === undefined) {
      throw new DisposedNodeError(handle.id);
    }
    if (!(node instanceof AsyncCellNode)) {
      throw new Error(`set_cell on non-cell id ${handle.id}`);
    }
    if (defaultEqual(node.value, value)) {
      return;
    }
    node.value = value;
    if (this.#batchDepth > 0) {
      this.#batchedCells.add(handle.id);
      return;
    }
    this.#invalidateCellDependents(handle.id);
  }

  /** @deprecated use {@link AsyncContext#set} — the unified cell write (#lzcellkernel). */
  setCell(handle, value) {
    return this.set(handle, value);
  }

  // -- Slot reads --------------------------------------------------------

  isResolved(handle) {
    const node = this.#nodes[handle.id];
    return node instanceof AsyncSlotNode && node.state === "resolved";
  }

  /** Public projection of the slot state machine (AsyncSlotStateView). */
  slotState(handle) {
    const node = this.#nodes[handle.id];
    if (!(node instanceof AsyncSlotNode)) {
      return "none";
    }
    return node.state;
  }

  /**
   * Await a slot value. Uses the synchronous `get()` fast path for resolved
   * slots; otherwise attaches to the single in-flight compute for the current
   * revision, or spawns one. Re-resolves through both benign race windows
   * (resolved-since-get, notifier-dropped) rather than asserting.
   */
  async getAsync(handle) {
    const id = handle.id;
    // Outer re-resolve loop: slot state is authoritative.
    // eslint-disable-next-line no-constant-condition
    while (true) {
      const node = this.#nodes[id];
      if (node === undefined) {
        throw new DisposedNodeError(id);
      }
      if (!(node instanceof AsyncSlotNode)) {
        throw new Error(`get_async on non-slot id ${id}`);
      }
      if (node.state === "resolved") {
        return node.value; // fast path (and benign window 1: resolved-since-get)
      }
      let notifier;
      if (node.state === "computing") {
        notifier = node.notifier;
      } else {
        // empty or error: spawn a fresh compute for the current revision.
        notifier = this.#spawnCompute(id);
      }
      const event = await notifier.promise;
      if (event.kind === "resolved") {
        return event.value;
      }
      if (event.kind === "error") {
        throw event.error;
      }
      // superseded (benign window 2): the world changed — re-resolve.
    }
  }

  getSignal(handle) {
    return this.get(handle.slot);
  }

  getSignalAsync(handle) {
    return this.getAsync(handle.slot);
  }

  // -- Dispose -----------------------------------------------------------

  async disposeAsyncEffect(handle) {
    const id = handle.id;
    const node = this.#nodes[id];
    if (!(node instanceof AsyncEffectNode)) {
      return;
    }
    // Remove pending reruns and abort any in-flight body first, then await the
    // current cleanup before removing the node.
    node.pending = false;
    if (node.abort) {
      node.abort.abort();
    }
    // Wait for the serialized run loop to quiesce so cleanup ordering holds.
    while (node.kicking) {
      await Promise.resolve();
      // Re-fetch: dispose may race the loop's own completion.
      if (!(this.#nodes[id] instanceof AsyncEffectNode)) {
        return;
      }
    }
    for (const dep of [...node.dependencies]) {
      this.#removeDependentEdge(dep, id);
    }
    const cleanup = node.cleanup;
    node.cleanup = null;
    this.#nodes[id] = undefined;
    this.#freeIds.push(id);
    if (typeof cleanup === "function") {
      await cleanup();
    }
  }

  isEffectActive(handle) {
    return this.#nodes[handle.id] instanceof AsyncEffectNode;
  }

  /**
   * Tear down an async derived slot (`#lzspecedgeindex`).
   *
   * Order is load-bearing and mirrors the synchronous core:
   *
   * 1. Mark the surviving dependent cone stale FIRST, while the edges still
   *    exist — once step 3 runs, nothing can reach those readers again. This is
   *    the step that is easy to omit and that leaves a live reader serving a
   *    value it computed *through* the node being torn down, forever, because
   *    with its edge gone nothing will ever invalidate it. `lazily-rs` shipped
   *    that defect (`5db90d2`) and so did this package's sync core (`4d20670`).
   * 2. Supersede any in-flight compute, so a completion that is already in
   *    flight cannot publish into a node that no longer exists.
   * 3. Detach BOTH edge directions — upstream so the source's dependent list
   *    stops growing (the leak disposal exists for), downstream so no survivor
   *    holds a dangling half-edge to a freed id.
   *
   * Idempotent, and a no-op on a handle of the wrong kind.
   */
  disposeSlot(handle) {
    const id = handle.id;
    const node = this.#nodes[id];
    if (!(node instanceof AsyncSlotNode)) {
      return;
    }
    this.#invalidateDisposedDependents(node.dependents);
    if (node.state === "computing") {
      node.token.aborted = true;
      if (node.abort) {
        node.abort.abort();
      }
      node.notifier.settle({ kind: "superseded" });
      this.#inflight.delete(id);
    }
    for (const dep of [...node.dependencies]) {
      this.#removeDependentEdge(dep, id);
    }
    for (const dependent of [...node.dependents]) {
      this.#removeDependencyEdge(dependent, id);
    }
    node.state = "empty";
    node.hasValue = false;
    node.token = null;
    node.abort = null;
    node.notifier = null;
    this.#nodes[id] = undefined;
    this.#freeIds.push(id);
  }

  /**
   * Tear down a source cell. Cells are pure sources with no dependencies, so
   * only downstream edges are detached — but the surviving cone is still marked
   * stale, or a dependent that cached a value read through this cell would serve
   * it forever instead of failing. See {@link disposeSlot}.
   */
  disposeCell(handle) {
    const id = handle.id;
    const node = this.#nodes[id];
    if (!(node instanceof AsyncCellNode)) {
      return;
    }
    this.#invalidateDisposedDependents(node.dependents);
    for (const dependent of [...node.dependents]) {
      this.#removeDependencyEdge(dependent, id);
    }
    this.#nodes[id] = undefined;
    this.#freeIds.push(id);
  }

  /**
   * Tear down whatever kind of node `handle` names. Dispatch is on the HANDLE'S
   * CLASS, not on the node currently at that id: ids are recycled here, and a
   * stale handle whose id has been reissued must be a no-op rather than tear
   * down the innocent new occupant (`dispose_stale_handle`).
   *
   * Returns a promise because effect teardown awaits the effect's run loop and
   * its cleanup; slot and cell teardown are synchronous underneath.
   */
  async disposeNode(handle) {
    if (handle instanceof AsyncSource) {
      this.disposeCell(handle);
    } else if (handle instanceof AsyncComputed) {
      this.disposeSlot(handle);
    } else if (handle instanceof AsyncEffectHandle) {
      await this.disposeAsyncEffect(handle);
    } else if (handle instanceof AsyncSignalHandle) {
      await this.disposeAsyncEffect(handle.effect);
      this.disposeSlot(handle.slot);
    } else {
      throw new TypeError("disposeNode: not a lazily async node handle");
    }
  }

  // -- Degree introspection (`#lzspecedgeindex`) -------------------------
  //
  // Counts, never collections — see the note on the synchronous core. The point
  // is that graph shape is assertable ("this churn cycle left the source's live
  // subscriber count where it started") with no path to the edge arrays and no
  // storage strategy pinned into the contract.

  #nodeIdOf(handle) {
    return handle.slot !== undefined ? handle.slot.id : handle.id;
  }

  /** How many nodes currently depend on `handle`. 0 for a disposed node and for
   * effects, which are pure sinks. */
  dependentCount(handle) {
    const node = this.#nodes[this.#nodeIdOf(handle)];
    if (node instanceof AsyncCellNode || node instanceof AsyncSlotNode) {
      return node.dependents.length;
    }
    return 0;
  }

  /** How many nodes `handle` currently depends on. 0 for a disposed node and for
   * cells, which are pure sources. */
  dependencyCount(handle) {
    const node = this.#nodes[this.#nodeIdOf(handle)];
    if (node instanceof AsyncSlotNode || node instanceof AsyncEffectNode) {
      return node.dependencies.length;
    }
    return 0;
  }

  /** Whether `handle`'s id currently names no live node. Ids are recycled, so
   * this means "disposed and not yet reused". */
  isNodeDisposed(handle) {
    return this.#nodes[this.#nodeIdOf(handle)] === undefined;
  }

  // -- Teardown scopes ---------------------------------------------------

  /**
   * Open an {@link AsyncTeardownScope}: nodes created through it are disposed
   * when it ends, in reverse creation order. Scoping bounds teardown, never
   * visibility.
   */
  scope() {
    return new AsyncTeardownScope(this);
  }

  /**
   * Run `body` with a fresh scope and end it in a `finally`. `body` may be
   * async; the scope ends after it settles.
   */
  async withScope(body) {
    const s = new AsyncTeardownScope(this);
    try {
      return await body(s);
    } finally {
      await s.end();
    }
  }

  disposeSignal(handle) {
    return this.disposeAsyncEffect(handle.effect);
  }

  isSignalActive(handle) {
    return this.isEffectActive(handle.effect);
  }

  /** Dispose the whole context: abort every in-flight compute and effect body,
   * then await all active effect cleanups. */
  async dispose() {
    const nodes = this.#nodes;
    for (let id = 0; id < nodes.length; id++) {
      const node = nodes[id];
      if (node === undefined) {
        continue;
      }
      if (node instanceof AsyncSlotNode && node.state === "computing") {
        node.token.aborted = true;
        if (node.abort) {
          node.abort.abort();
        }
        node.notifier.settle({ kind: "superseded" });
        this.#inflight.delete(id);
      } else if (node instanceof AsyncEffectNode) {
        node.pending = false;
        if (node.abort) {
          node.abort.abort();
        }
      }
    }
    await this.settle();
    const cleanups = [];
    for (let id = 0; id < nodes.length; id++) {
      const node = nodes[id];
      if (node instanceof AsyncEffectNode && typeof node.cleanup === "function") {
        cleanups.push(node.cleanup);
        node.cleanup = null;
      }
    }
    for (const cleanup of cleanups) {
      await cleanup();
    }
  }

  // -- Batch -------------------------------------------------------------

  batch(run) {
    this.#batchDepth++;
    try {
      run();
    } finally {
      this.#batchDepth--;
      if (this.#batchDepth === 0) {
        const cells = [...this.#batchedCells];
        this.#batchedCells.clear();
        for (const id of cells) {
          this.#invalidateCellDependents(id);
        }
      }
    }
  }

  /** Await all scheduled effect reruns and in-flight computes until the graph
   * quiesces. Deterministic anchor for tests and for eager-signal materialization
   * (async reruns settle on the executor, not synchronously in `set`). */
  async settle() {
    while (this.#effectRuns.size > 0 || this.#inflight.size > 0) {
      await Promise.allSettled([...this.#effectRuns, ...this.#inflight.values()]);
    }
  }

  // -- Internals: ids + edges -------------------------------------------

  #allocId() {
    return this.#freeIds.pop() ?? this.#nextId++;
  }

  #registerDependency(depId, ownerId) {
    const dep = this.#nodes[depId];
    // #lzspecedgeindex: a read that lands on a disposed node must leave NO edge
    // behind. The owner-side insert below is unconditional otherwise, so without
    // this the freed id would be pushed onto the reader's own `dependencies`
    // list and outlive the `DisposedNodeError` the read is about to throw —
    // a dangling half-edge, which is exactly what `dependencyCount` exists to
    // rule out. Mirrors the pre-`_track` check in the synchronous core.
    if (dep === undefined) {
      return;
    }
    if (dep instanceof AsyncCellNode || dep instanceof AsyncSlotNode) {
      edgeInsert(dep.dependents, ownerId);
    }
    const owner = this.#nodes[ownerId];
    if (owner instanceof AsyncSlotNode || owner instanceof AsyncEffectNode) {
      edgeInsert(owner.dependencies, depId);
    }
  }

  #removeDependentEdge(depId, ownerId) {
    const dep = this.#nodes[depId];
    if (dep instanceof AsyncCellNode || dep instanceof AsyncSlotNode) {
      edgeRemove(dep.dependents, ownerId);
    }
  }

  // Symmetric to #removeDependentEdge: drop `depId` from the consumer-side list
  // of `ownerId`. Only disposal needs it — every other path clears an owner's
  // whole dependency list at once before re-tracking.
  #removeDependencyEdge(ownerId, depId) {
    const owner = this.#nodes[ownerId];
    if (owner instanceof AsyncSlotNode || owner instanceof AsyncEffectNode) {
      edgeRemove(owner.dependencies, depId);
    }
  }

  // A compute/effect callback receives this context. Dependency edges register
  // synchronously, BEFORE the awaited read, so a source invalidation while the
  // future is suspended supersedes the in-flight compute before it publishes.
  #makeComputeContext(ownerId, signal) {
    const self = this;
    return {
      signal,
      // Unified cell read inside an async compute (#lzcellkernel): tracks the
      // dependency, then reads the source value / slot snapshot.
      get(handle) {
        self.#registerDependency(handle.id, ownerId);
        return self.get(handle);
      },
      /** @deprecated use `get` — the unified cell read (#lzcellkernel). */
      getCell(handle) {
        return this.get(handle);
      },
      getAsync(handle) {
        self.#registerDependency(handle.id, ownerId);
        return self.getAsync(handle);
      },
      getSignalAsync(handle) {
        self.#registerDependency(handle.slot.id, ownerId);
        return self.getAsync(handle.slot);
      },
    };
  }

  // -- Internals: slot compute ------------------------------------------

  #spawnCompute(id) {
    const node = this.#nodes[id];
    // Clear old dependency edges; they are rediscovered live during compute.
    for (const dep of [...node.dependencies]) {
      this.#removeDependentEdge(dep, id);
    }
    node.dependencies.length = 0;

    const token = { revision: node.revision, aborted: false };
    const controller = new AbortController();
    const notifier = makeNotifier();
    node.state = "computing";
    node.hasValue = false;
    node.token = token;
    node.abort = controller;
    node.notifier = notifier;

    const cctx = this.#makeComputeContext(id, controller.signal);
    const task = Promise.resolve()
      .then(() => node.compute(cctx))
      .then(
        (value) => this.#publishResolved(id, token, notifier, value),
        (error) => this.#publishError(id, token, notifier, error),
      )
      .finally(() => {
        this.#inflight.delete(id);
      });
    this.#inflight.set(id, task);
    return notifier;
  }

  #publishResolved(id, token, notifier, value) {
    const node = this.#nodes[id];
    if (!(node instanceof AsyncSlotNode) || token.aborted || node.revision !== token.revision) {
      // Stale completion: discard, never publish. Waiters re-resolve.
      notifier.settle({ kind: "superseded" });
      return;
    }
    // Propagation is eager at the mutation boundary: `set` -> `#invalidateSlot`
    // already cascaded to the whole transitive dependent subtree (slots -> empty,
    // effects -> rescheduled), so dependents re-pull the new value on their next
    // read. Publishing does not re-propagate. The `memo` equality guard keeps the
    // cached value stable but, per the async contract, does not suppress the
    // force-rerun of dependents that the invalidation cascade already scheduled.
    node.state = "resolved";
    node.value = value;
    node.hasValue = true;
    node.error = undefined;
    node.token = null;
    node.abort = null;
    node.notifier = null;
    notifier.settle({ kind: "resolved", value });
  }

  #publishError(id, token, notifier, error) {
    const node = this.#nodes[id];
    if (!(node instanceof AsyncSlotNode) || token.aborted || node.revision !== token.revision) {
      notifier.settle({ kind: "superseded" });
      return;
    }
    node.state = "error";
    node.error = error;
    node.hasValue = false;
    node.token = null;
    node.abort = null;
    node.notifier = null;
    notifier.settle({ kind: "error", error });
  }

  // -- Internals: invalidation ------------------------------------------

  #invalidateCellDependents(id) {
    const node = this.#nodes[id];
    if (!(node instanceof AsyncCellNode)) {
      return;
    }
    for (const d of [...node.dependents]) {
      this.#invalidateDependent(d);
    }
  }

  // Mark the cone left behind by a disposal stale, without scheduling anything
  // (`#lzspecedgeindex`). Reuses `#invalidateDependent` — the same walk every
  // publish takes — rather than a second traversal, so there is exactly one
  // definition of "transitively reached" in this graph and the two cannot drift.
  #invalidateDisposedDependents(dependents) {
    if (dependents.length === 0) {
      return;
    }
    this.#disposalDepth++;
    try {
      for (const d of [...dependents]) {
        this.#invalidateDependent(d);
      }
    } finally {
      this.#disposalDepth--;
    }
  }

  #invalidateDependent(id) {
    const node = this.#nodes[id];
    if (node instanceof AsyncEffectNode) {
      this.#scheduleEffect(id);
    } else if (node instanceof AsyncSlotNode) {
      this.#invalidateSlot(id);
    }
  }

  #invalidateSlot(id) {
    const node = this.#nodes[id];
    if (!(node instanceof AsyncSlotNode)) {
      return;
    }
    node.revision++;
    if (node.state === "computing") {
      // Supersede the in-flight compute: abort it and release its waiters so
      // they re-resolve against the new revision.
      node.token.aborted = true;
      if (node.abort) {
        node.abort.abort();
      }
      node.notifier.settle({ kind: "superseded" });
      this.#inflight.delete(id);
      node.abort = null;
      node.notifier = null;
      node.token = null;
    }
    node.state = "empty";
    node.hasValue = false;
    // Cascade to this slot's dependents (lazy slots go empty; effects reschedule).
    for (const d of [...node.dependents]) {
      this.#invalidateDependent(d);
    }
  }

  // -- Internals: async effects (serialized, cleanup-before-body) --------

  #scheduleEffect(id) {
    // #lzspecedgeindex: disposal is not a publish. See `#disposalDepth`.
    if (this.#disposalDepth > 0) {
      return;
    }
    const node = this.#nodes[id];
    if (!(node instanceof AsyncEffectNode)) {
      return;
    }
    node.pending = true;
    if (node.kicking) {
      // A run loop is already active; it will pick up the rerun after the
      // current body (and its cleanup) finish. Reruns are serialized.
      return;
    }
    node.kicking = true;
    // Start the run loop on a microtask, NOT synchronously: all cell writes in
    // the current synchronous pass (e.g. inside a `batch`) coalesce into a single
    // `pending` flag before the body runs, so a batch triggers exactly one rerun.
    const run = Promise.resolve().then(async () => {
      try {
        while (node.pending && this.#nodes[id] === node) {
          node.pending = false;
          await this.#runEffectOnce(id);
        }
      } finally {
        node.kicking = false;
      }
    });
    this.#effectRuns.add(run);
    run.finally(() => this.#effectRuns.delete(run));
  }

  async #runEffectOnce(id) {
    const node = this.#nodes[id];
    if (!(node instanceof AsyncEffectNode)) {
      return;
    }
    // Cleanup of the previous body completes before the next body starts.
    if (typeof node.cleanup === "function") {
      const cleanup = node.cleanup;
      node.cleanup = null;
      await cleanup();
    }
    if (this.#nodes[id] !== node) {
      return; // disposed during cleanup
    }
    for (const dep of [...node.dependencies]) {
      this.#removeDependentEdge(dep, id);
    }
    node.dependencies.length = 0;
    const controller = new AbortController();
    node.abort = controller;
    const cctx = this.#makeComputeContext(id, controller.signal);
    let nextCleanup;
    try {
      nextCleanup = await node.run(cctx);
    } catch (error) {
      if (!controller.signal.aborted) {
        node.lastError = error;
      }
      return;
    } finally {
      if (node.abort === controller) {
        node.abort = null;
      }
    }
    const current = this.#nodes[id];
    if (current instanceof AsyncEffectNode) {
      current.cleanup = typeof nextCleanup === "function" ? nextCleanup : null;
    } else if (typeof nextCleanup === "function") {
      await nextCleanup(); // disposed mid-run: run the fresh cleanup immediately
    }
  }
}

// See the note on `DISPOSE` in ./reactive.js: a computed class member, not a
// post-class prototype assignment, so this stays tree-shakeable.
const ASYNC_DISPOSE = Symbol.asyncDispose ?? Symbol.for("Symbol.asyncDispose");

/**
 * A teardown scope over an {@link AsyncContext}: nodes created through it are
 * disposed when it ends (`#lzspecedgeindex`).
 *
 * The async twin of `./reactive.js`'s `TeardownScope`, and the shape that makes
 * the explicit end obviously right rather than merely necessary. Async effect
 * teardown awaits the effect's run loop and its cleanup, so `end()` is a promise
 * — and the scopes that matter most here are exactly the ones no lexical bracket
 * can express: a scope whose lifetime is a websocket connection or a
 * subscription, opened in one callback and ended in another. `withScope` is
 * offered for the lexical case; `Symbol.asyncDispose` is installed where the
 * runtime defines it, so `await using scope = ctx.scope()` also works.
 *
 * A GC finalizer is deliberately NOT offered — see the sync class comment.
 *
 * Teardown runs in REVERSE creation order, and each node is awaited before the
 * next is torn down. Graph state does not depend on that order, but effect
 * cleanups are side effects and their order is observable; serializing them is
 * what makes `disposeScope_eq_disposeAll` hold for the async graph too.
 */
export class AsyncTeardownScope {
  #ctx;
  #owned = [];
  #ended = false;

  /** @internal — obtain one from {@link AsyncContext#scope}. */
  constructor(ctx) {
    this.#ctx = ctx;
  }

  /** How many nodes this scope currently owns. */
  get size() {
    return this.#owned.length;
  }

  /** Whether {@link end} has already run. */
  get ended() {
    return this.#ended;
  }

  /** Take ownership of an existing node. No-op on an already-ended scope: the
   * scope's moment has passed, and adopting is not a request to dispose now. */
  adopt(handle) {
    if (!this.#ended) {
      this.#owned.push(handle);
    }
    return handle;
  }

  /** Create a source cell owned by this scope (#lzcellkernel). */
  source(value) {
    return this.adopt(this.#ctx.source(value));
  }

  /** @deprecated use {@link AsyncTeardownScope#source}. */
  cell(value) {
    return this.adopt(this.#ctx.source(value));
  }

  /** Create an async computed slot owned by this scope. */
  computedAsync(compute) {
    return this.adopt(this.#ctx.computedAsync(compute));
  }

  /** Create an equality-guarded async memo owned by this scope. */
  memoAsync(compute) {
    return this.adopt(this.#ctx.memoAsync(compute));
  }

  /** Create an eager async signal owned by this scope. */
  signalAsync(compute) {
    return this.adopt(this.#ctx.signalAsync(compute));
  }

  /** Register an async effect owned by this scope. */
  effectAsync(run) {
    return this.adopt(this.#ctx.effectAsync(run));
  }

  /**
   * Cancel this scope's teardown: ending it afterwards disposes nothing, and its
   * nodes revert to plain context ownership. The nodes themselves are untouched
   * — same values, same edges, still individually disposable. Only whether this
   * scope fires at end-of-life changes.
   */
  disarm() {
    this.#owned.length = 0;
  }

  /** Dispose every node this scope owns, in reverse creation order. Idempotent. */
  async end() {
    if (this.#ended) {
      return;
    }
    this.#ended = true;
    const owned = this.#owned;
    for (let i = owned.length - 1; i >= 0; i--) {
      await this.#ctx.disposeNode(owned[i]);
    }
    owned.length = 0;
  }

  /** TC39 explicit resource management: `await using scope = ctx.scope()`. */
  [ASYNC_DISPOSE]() {
    return this.end();
  }
}
