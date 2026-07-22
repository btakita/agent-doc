// RelayCell backpressure plan (#relaycell), Phases 2–6 — the stackless JS port.
//
// See lazily-spec/docs/relaycell.md and relaycell-backpressure-analysis.md. A
// RelayCell is an *algebra-typed conflating relay*: it accumulates a fast ingress
// into a hot head (a MergePolicy fold), bounds it with a reactive
// BackpressurePolicy, and lets a slow egress drain the coalesced window. The
// converged egress state is independent of the drain schedule whenever the merge
// ⊕ is associative (the relay_converges invariant, pinned in LazilyFormal.Relay).
//
// Phase 2 RelayCell + BackpressurePolicy · Phase 3 SpillStore · Phase 4 Transport
// · Phase 5 Outbox/Inbox roles · Phase 6 Rate/Window/Expiry/Priority/Keyed
// policies. Time is a logical clock (a monotone tick) so behaviour is
// deterministic and portable.

// -- Phase 2: RelayCell + BackpressurePolicy ---------------------------------

/** What a bound measures (analysis §4.4). The core meters Count. */
export const BoundDim = Object.freeze({
  Count: "Count",
  Bytes: "Bytes",
  Keys: "Keys",
  Age: "Age",
});

/** The action taken when the hot head crosses high_water (analysis §4.4). */
export const Overflow = Object.freeze({
  /** Refuse ingress; the producer backpressures (observes is_full). Lossless. */
  Block: "Block",
  /** Discard the incoming op. Lossy. */
  DropNewest: "DropNewest",
  /** Reset the window to the incoming op, discarding what accumulated. Lossy. */
  DropOldest: "DropOldest",
  /** Keep merging — the coalescence *is* the bound. Requires policy.conflates. */
  Conflate: "Conflate",
  /** Page the accumulated window to a durable tail (Phase 3 SpillStore). */
  Spill: "Spill",
});

/** Why a construction/merge-swap was rejected (analysis §4.3 flag validation). */
export const RelayConfigError = Object.freeze({
  /** Conflate chosen for a non-conflating policy (RawFifo). */
  ConflateNotBounding: "ConflateNotBounding",
});

/** The outcome of a single ingress op. */
export const IngressOutcome = Object.freeze({
  /** Merged into an empty window (window depth was 0). */
  Accepted: "Accepted",
  /** Merged into a non-empty window (coalesced with prior ops). */
  Conflated: "Conflated",
  /** Dropped by DropNewest/DropOldest overflow. */
  Dropped: "Dropped",
  /** Refused by Block overflow; the producer must retry after a drain. */
  Blocked: "Blocked",
});

/**
 * Reactive backpressure limits (analysis §4.4). Every field is a cell, so an
 * operator or an adaptive controller retunes it live and dependent relays react.
 * Hysteresis (high_water ≠ low_water) prevents flapping.
 */
export class BackpressurePolicy {
  /** @param {import("./reactive.js").Context} ctx */
  constructor(ctx, dimension, highWater, lowWater, overflow) {
    this.ctx = ctx;
    this.dimension = ctx.source(dimension);
    this.highWater = ctx.source(highWater);
    this.lowWater = ctx.source(lowWater);
    this.overflow = ctx.source(overflow);
  }
}

/**
 * The algebra-typed conflating relay (Phase 2, in-proc core). The hot head is a
 * cell; depth/is_full/is_empty are demand-driven slots, so an unobserved relay
 * costs N·⊕ and no more (the merge cost law).
 */
export class RelayCell {
  /**
   * @param {import("./reactive.js").Context} ctx
   * @param {BackpressurePolicy} policy
   * Throws RelayConfigError.ConflateNotBounding if Conflate is chosen for a
   * non-conflating policy.
   */
  constructor(ctx, policy, mergePolicy) {
    if (ctx.get(policy.overflow) === Overflow.Conflate && !mergePolicy.conflates) {
      throw new Error(RelayConfigError.ConflateNotBounding);
    }
    this.ctx = ctx;
    this.policy = policy;
    this.mergePolicy = mergePolicy;
    // Hot head: current window's coalesced value (null = empty window).
    this._head = ctx.source(null);
    // Ops merged into the current window since the last drain (the Count bound).
    this._pending = ctx.source(0);
    this._depth = ctx.computed((c) => c.get(this._pending));
    this._isFull = ctx.computed(
      (c) => c.get(this._pending) >= c.get(policy.highWater),
    );
    this._isEmpty = ctx.computed((c) => c.get(this._head) === null);
  }

  /** Whether the current overflow choice is legal for the policy. */
  overflowIsLegal() {
    return (
      this.ctx.get(this.policy.overflow) !== Overflow.Conflate ||
      this.mergePolicy.conflates
    );
  }

  /** Demand-driven reader: current window depth (Count). */
  depth() {
    return this.ctx.get(this._depth);
  }
  /** Demand-driven reader: window is at/over high_water. */
  isFull() {
    return this.ctx.get(this._isFull);
  }
  /** Demand-driven reader: window is empty (nothing to drain). */
  isEmpty() {
    return this.ctx.get(this._isEmpty);
  }

  /** The reader slot handles (for wiring into effects/computations). */
  depthSlot() {
    return this._depth;
  }
  isFullSlot() {
    return this._isFull;
  }
  isEmptySlot() {
    return this._isEmpty;
  }

  _readFull() {
    return this.ctx.get(this._pending) >= this.ctx.get(this.policy.highWater);
  }

  _mergeIntoHead(op) {
    const cur = this.ctx.get(this._head);
    const next = cur === null ? op : this.mergePolicy.merge(cur, op);
    this.ctx.set(this._head, next);
  }

  /**
   * Ingest one op. Applies the reactive overflow policy when the window is at
   * high_water; otherwise merges the op into the hot head under the policy.
   */
  ingress(op) {
    const wasEmpty = this.ctx.get(this._pending) === 0;
    if (this._readFull()) {
      switch (this.ctx.get(this.policy.overflow)) {
        case Overflow.Block:
          return IngressOutcome.Blocked;
        case Overflow.DropNewest:
          return IngressOutcome.Dropped;
        case Overflow.DropOldest:
          this.ctx.set(this._head, op);
          this.ctx.set(this._pending, 1);
          return IngressOutcome.Dropped;
        // Conflate keeps merging; Spill is Phase 3 and, until wired, degrades to
        // Conflate for a bounding policy. Both fall through to the merge below.
        case Overflow.Conflate:
        case Overflow.Spill:
          break;
      }
    }
    this._mergeIntoHead(op);
    this.ctx.set(this._pending, this.ctx.get(this._pending) + 1);
    return wasEmpty ? IngressOutcome.Accepted : IngressOutcome.Conflated;
  }

  /**
   * Drain the coalesced window: take the hot head's value and reset the window.
   * Returns null for an empty window. relay_converges guarantees the egress fold
   * equals the flat fold of every ingested op, for any drain schedule.
   */
  drain() {
    const cur = this.ctx.get(this._head);
    if (cur !== null) {
      this.ctx.set(this._head, null);
      this.ctx.set(this._pending, 0);
    }
    return cur;
  }

  /** Peek the current coalesced window without draining. */
  peek() {
    return this.ctx.get(this._head);
  }
}

// -- Phase 3: SpillStore -----------------------------------------------------

/** How spilled windows are laid out on the durable tail (analysis §6). */
export const SpillMode = Object.freeze({
  /** Merge each spilled window into the open page until it fills — minimizes
   *  disk (keep-latest / semilattice). One page holds a coalesced run. */
  CompactOnWrite: "CompactOnWrite",
  /** Append each spilled window as its own page — preserves increments for an
   *  accumulating (non-idempotent) policy that must not double-count. */
  AppendCompact: "AppendCompact",
});

/**
 * A paged durable tail for a RelayCell (Phase 3, in-memory reference backend).
 * Holds a hot page in RAM plus immutable cold pages, a bounded manifest, an
 * egress cursor, and ack-before-reclaim. Memory is O(hot) + O(manifest).
 */
export class SpillStore {
  constructor(mode, pageSize, mergePolicy) {
    this.mode = mode;
    this.pageSize = Math.max(1, pageSize);
    this.mergePolicy = mergePolicy;
    /** @type {{id:number, summary:*, bytes:number}[]} immutable cold pages */
    this._pages = [];
    this._openFill = 0;
    this._nextId = 0;
    this._acked = 0; // pages acked from the front (reclaimable)
  }

  /**
   * Spill one coalesced window summary to the durable tail. AppendCompact always
   * opens a new page; CompactOnWrite merges into the open page until it reaches
   * page_size, then seals it.
   */
  spill(window, bytes) {
    if (this.mode === SpillMode.AppendCompact) {
      this._pushPage(window, bytes);
    } else {
      if (this._openFill >= this.pageSize || this._pages.length === 0) {
        this._pushPage(window, bytes);
        this._openFill = 1;
      } else {
        const last = this._pages[this._pages.length - 1];
        last.summary = this.mergePolicy.merge(last.summary, window);
        last.bytes += bytes;
        this._openFill += 1;
      }
    }
  }

  _pushPage(summary, bytes) {
    this._pages.push({ id: this._nextId, summary, bytes });
    this._nextId += 1;
  }

  /** The manifest: [id, bytes] for every live page (bounded metadata). */
  manifest() {
    return this._pages.map((p) => [p.id, p.bytes]);
  }

  /** Pages the egress has not yet acked (at/after the ack cursor). */
  pendingPages() {
    return this._pages.slice(this._acked);
  }

  pageCount() {
    return this._pages.length;
  }

  /** Ack every page through id (inclusive), advancing the reclaim cursor. */
  ackThrough(id) {
    while (this._acked < this._pages.length && this._pages[this._acked].id <= id) {
      this._acked += 1;
    }
  }

  /** Drop acked pages (durable reclaim). Manifest/cursor stay consistent. */
  reclaim() {
    if (this._acked > 0) {
      this._pages.splice(0, this._acked);
      this._acked = 0;
    }
  }

  /** Fold every live cold page (oldest first) into s0. */
  foldPages(s0) {
    return this._pages.reduce((acc, p) => this.mergePolicy.merge(acc, p.summary), s0);
  }

  /**
   * Reconstruction (spill_lossless). Fold the cold tail then the hot head —
   * reproduces the flat fold of every op the relay ever ingested.
   */
  reconstruct(s0, hot) {
    const cold = this.foldPages(s0);
    return hot === null || hot === undefined ? cold : this.mergePolicy.merge(cold, hot);
  }

  /**
   * Crash replay. After recovery the egress re-delivers every unacked page from
   * the ack cursor into downstream. For an idempotent policy re-applying an
   * already-delivered page is a no-op (spill_replay_idempotent), so at-least-once
   * replay converges to the same downstream state.
   */
  replayUnacked(downstream) {
    return this.pendingPages().reduce(
      (acc, p) => this.mergePolicy.merge(acc, p.summary),
      downstream,
    );
  }
}

// -- Phase 4: Transport ------------------------------------------------------
//
// Transport abstracts ingress/egress delivery so the mechanism is pluggable. A
// RelayCell is written once against Transport; the merge algebra — not the
// transport — guarantees converged state (transport_independent), so transports
// may differ across bindings and still converge.

/** InProc — direct delivery: every buffered op is handed over in one frame. */
export class InProcTransport {
  constructor() {
    this._buf = [];
  }
  deliver(op) {
    this._buf.push(op);
  }
  poll() {
    const out = this._buf;
    this._buf = [];
    return out;
  }
  hasPending() {
    return this._buf.length > 0;
  }
}

/**
 * A framed transport — models CrossThread/Ipc/Ws: ops are delivered in bounded
 * frames of at most frameSize (an MTU / batch boundary). Different frameSizes are
 * different framings of the same op stream.
 */
export class FramedTransport {
  constructor(frameSize) {
    this._buf = [];
    this.frameSize = Math.max(1, frameSize);
  }
  deliver(op) {
    this._buf.push(op);
  }
  poll() {
    const n = Math.min(this.frameSize, this._buf.length);
    return this._buf.splice(0, n);
  }
  hasPending() {
    return this._buf.length > 0;
  }
}

// -- Phase 5: Outbox / Inbox roles -------------------------------------------
//
// RelayCell is direction-neutral; Outbox and Inbox are role facades (typed
// constructors with direction-appropriate defaults), not reimplementations. They
// differ in the backpressure-propagation contract. A network link is
// Outbox → Transport → Inbox.

/**
 * The app → transport send side (analysis §4.7). Backpressures the local
 * producer directly via is_full. Default overflow Conflate (state broadcast).
 */
export class Outbox {
  /**
   * @param {import("./reactive.js").Context} ctx
   * Build an outbox bounded by highWater. Optional dimension/overflow (e.g.
   * Spill for a lossless event channel).
   */
  constructor(ctx, highWater, mergePolicy, opts = {}) {
    const dimension = opts.dimension ?? BoundDim.Count;
    const overflow = opts.overflow ?? Overflow.Conflate;
    const policy = new BackpressurePolicy(
      ctx,
      dimension,
      highWater,
      Math.floor(highWater / 2),
      overflow,
    );
    this._relay = new RelayCell(ctx, policy, mergePolicy);
  }

  /** The local producer sends an op. A Blocked outcome is the producer's
   *  backpressure signal — it should await a drain before retrying. */
  send(op) {
    return this._relay.ingress(op);
  }

  /** The transport drains the coalesced window for egress. */
  drain() {
    return this._relay.drain();
  }

  /** The producer-facing backpressure signal (window at/over the watermark). */
  isFull() {
    return this._relay.isFull();
  }
  isFullSlot() {
    return this._relay.isFullSlot();
  }

  /** Access the underlying relay (for wiring extra egress stages). */
  relay() {
    return this._relay;
  }
}

/**
 * The transport → app receive side (analysis §4.7). Cannot block the remote
 * directly; backpressure is a credit meter the app replenishes.
 */
export class Inbox {
  /**
   * @param {import("./reactive.js").Context} ctx
   * Build an inbox bounded by highWater with a credit budget of maxCredits.
   */
  constructor(ctx, highWater, maxCredits, mergePolicy, opts = {}) {
    const overflow = opts.overflow ?? Overflow.Conflate;
    const policy = new BackpressurePolicy(
      ctx,
      BoundDim.Count,
      highWater,
      Math.floor(highWater / 2),
      overflow,
    );
    this._relay = new RelayCell(ctx, policy, mergePolicy);
    this._credits = maxCredits;
    this._maxCredits = maxCredits;
  }

  /** Whether the transport may deliver another message (a credit is available).
   *  When false, the transport must stop reading → the remote throttles. */
  ready() {
    return this._credits > 0;
  }

  /** Credits currently available to the remote. */
  credits() {
    return this._credits;
  }

  /** The transport delivers a received op. Consumes a credit; the caller MUST
   *  have checked ready() (a delivery without credit still applies but drives
   *  credits to zero, signalling the remote to stop). */
  receive(op) {
    this._credits = Math.max(0, this._credits - 1);
    return this._relay.ingress(op);
  }

  /** The app consumes the coalesced window and replenishes n credits (up to the
   *  budget), re-opening the remote's flow. */
  consume(replenish) {
    const out = this._relay.drain();
    this._credits = Math.min(this._credits + replenish, this._maxCredits);
    return out;
  }
}

// -- Phase 6: extra reactive policies ----------------------------------------
//
// Each policy is an optional reactive stage composed onto a relay egress; they
// only change where/when a relay flushes or which ops survive. Time is a logical
// clock (a monotone tick) — a binding drives tick/advance from its own runtime
// timer.

/**
 * Case 9 — rate-limited egress (token bucket). A drain is permitted only when a
 * token is available. Refilled refillPerTick tokens per logical tick, capped at
 * capacity.
 */
export class RatePolicy {
  constructor(capacity, refillPerTick) {
    this.capacity = capacity;
    this._tokens = capacity;
    this.refillPerTick = refillPerTick;
  }
  tokens() {
    return this._tokens;
  }
  /** Try to consume one token for an egress; returns true if paced through. */
  tryEgress() {
    if (this._tokens > 0) {
      this._tokens -= 1;
      return true;
    }
    return false;
  }
  /** Advance the logical clock, refilling the bucket (saturating at capacity). */
  tick() {
    this._tokens = Math.min(this._tokens + this.refillPerTick, this.capacity);
  }
}

/**
 * Case 8 — time-windowed coalescence (debounce/throttle). Flushes when it reaches
 * windowOps ops or on an explicit tick. Because a window is just a flush group,
 * associativity keeps the converged state unchanged (flushGroupingIrrelevant).
 */
export class WindowPolicy {
  constructor(windowOps) {
    this.windowOps = Math.max(1, windowOps);
    this._pending = 0;
  }
  /** Record one ingress; returns true when the window is full and should flush. */
  onIngress() {
    this._pending += 1;
    if (this._pending >= this.windowOps) {
      this._pending = 0;
      return true;
    }
    return false;
  }
  /** The debounce/throttle interval elapsed: flush whatever is pending. */
  tick() {
    if (this._pending > 0) {
      this._pending = 0;
      return true;
    }
    return false;
  }
}

/**
 * Case 10 — TTL / deadline expiry. Drops elements whose age exceeds ttl against a
 * logical clock. Lossy-by-age (explicit); used to shed cold data.
 */
export class ExpiryPolicy {
  constructor(ttl) {
    this.ttl = ttl;
    this._now = 0;
  }
  advance(by) {
    this._now += by;
  }
  now() {
    return this._now;
  }
  /** Whether an element stamped at stampedAt is still live (not expired). */
  isLive(stampedAt) {
    return this._now - stampedAt <= this.ttl;
  }
  /** Retain only the live elements of a [ts, value] batch (drop the aged tail). */
  retainLive(batch) {
    return batch.filter(([ts]) => this.isLive(ts)).map(([, v]) => v);
  }
}

/**
 * Case 11 — priority egress. Ingress carries a priority; egress pops the highest
 * priority first (not FIFO), FIFO within equal priority. Reordering, so sound for
 * a commutative merge downstream (reorder_adjacent).
 */
export class PriorityStorage {
  constructor() {
    /** @type {{priority:number, seq:number, value:*}[]} */
    this._items = [];
    this._seq = 0;
  }
  push(priority, value) {
    this._items.push({ priority, seq: this._seq, value });
    this._seq += 1;
  }
  /** Pop the highest-priority element (FIFO within equal priority). */
  pop() {
    if (this._items.length === 0) return null;
    let best = 0;
    for (let i = 1; i < this._items.length; i++) {
      const a = this._items[i];
      const b = this._items[best];
      if (a.priority > b.priority || (a.priority === b.priority && a.seq < b.seq)) {
        best = i;
      }
    }
    const [out] = this._items.splice(best, 1);
    return out.value;
  }
  get length() {
    return this._items.length;
  }
  isEmpty() {
    return this._items.length === 0;
  }
}

/**
 * Case 18 — keyed sharding. N independent relays keyed by K; an op routes to its
 * key's shard. Merging across shards requires a commutative merge. The converged
 * per-key state equals a single relay per key.
 */
export class KeyedRelay {
  constructor(ctx, highWater, overflow, mergePolicy) {
    this.ctx = ctx;
    this.highWater = highWater;
    this.overflow = overflow;
    this.mergePolicy = mergePolicy;
    /** @type {Map<*, RelayCell>} */
    this._shards = new Map();
  }
  /** Route op to key's shard, creating the shard on first use. */
  ingress(key, op) {
    let relay = this._shards.get(key);
    if (relay === undefined) {
      const policy = new BackpressurePolicy(
        this.ctx,
        BoundDim.Count,
        this.highWater,
        Math.floor(this.highWater / 2),
        this.overflow,
      );
      relay = new RelayCell(this.ctx, policy, this.mergePolicy);
      this._shards.set(key, relay);
    }
    return relay.ingress(op);
  }
  /** Drain a key's coalesced window. */
  drain(key) {
    const relay = this._shards.get(key);
    return relay === undefined ? null : relay.drain();
  }
  keys() {
    return this._shards.keys();
  }
}
