// Reactive queue: QueueCell + pluggable QueueStorage backend (#lzqueue).
//
// Pure logic — no reactive graph. Like the keyed collections
// (`./collections.js`), this is compute that every binding MUST implement; the
// conformance/collections/queuecell_*.json fixtures pin behavior. To make a
// queue live-reactive, wrap its ops in a `Context` (cells/slots/effects) and
// use the returned `invalidates` matrix to drive reader-kind invalidation —
// see `lazily-spec/cell-model.md` § "Reactive queues" and the distributed-queue
// PRD for the shell / storage split.
//
// QueueCell is specified as a single-producer / single-consumer (SPSC)
// primitive; MPSC (multi-producer) is a *usage rule* on the same type —
// multiple producers push inside one logical batch, and the batch boundary
// serializes the pushes into a deterministic order. There is no separate
// MPSCQueueCell type (`lazily-spec/cell-model.md` § "QueueCell — SPSC
// primitive with MPSC usage rule").
//
// Invalidation is scoped to **reader kind**, not individual positions. A push
// invalidates `len` / `is_empty` readers (and `head` when transitioning from
// empty, `is_full` when transitioning onto capacity); a pop invalidates `head`
// / `len` / `is_empty` readers (and `is_full` when transitioning off capacity).
// The `invalidates` matrix returned by each mutating op reports exactly which
// reader kinds changed — the core reader-kind independence law, which mirrors
// the `PartialEq` guard the reactive bindings implement for free.

// ---------------------------------------------------------------------------
// Error sentinels — observable rejection labels (distinct observable signals).
// `Full` and `Closed` are the two push-rejection reasons; `Empty` and `Closed`
// are the two pop-rejection reasons. These match the cross-language conformance
// fixture `returns` labels (`lazily-spec/conformance/collections/queuecell_*`).
// ---------------------------------------------------------------------------

export const QueuePushError = Object.freeze({
  /** Bounded backend at capacity (reject policy on the default backend). */
  Full: "Full",
  /** Queue is closed; push is rejected regardless of capacity. Terminal. */
  Closed: "Closed",
});

export const QueuePopError = Object.freeze({
  /** Queue is open but contains no elements. */
  Empty: "Empty",
  /** Queue is closed and empty (drained). Distinct from `Empty`. */
  Closed: "Closed",
});

// ---------------------------------------------------------------------------
// QueueStorage contract (duck-typed).
// ---------------------------------------------------------------------------
//
// The shell / storage split keeps the reactive shell storage-agnostic. The
// default backend is `VecDequeStorage` (unbounded or bounded array-backed
// FIFO).
//
// Minimal required contract: a backend MUST implement `tryPush` / `tryPop` /
// `len` / `isClosed` / `close`. `peek` and `capacity` are OPTIONAL capabilities
// (default: absent) — a raw channel that satisfies only the five required
// methods is fully conforming; it simply has no `head` reader (no `peek`) and no
// `isFull` reader (unbounded, `capacity() → null`). A conforming backend MUST
// also:
//
// 1. FIFO order — `tryPop()` returns elements in `tryPush()` order.
// 2. Cardinality compatibility — native producer/consumer shape is a superset
//    of SPSC; MPSC usage requires a multi-writer backend.
// 3. Bounded contract (optional) — a bounded backend's `capacity()` returns a
//    number and `tryPush()` returns `QueuePushError.Full` at capacity.
// 4. Position identity — invalidation is phrased over reader kind, not storage
//    indices; the shell layers its own logical reader-kind derivations above
//    storage.

// ---------------------------------------------------------------------------
// VecDequeStorage — the reference unbounded/bounded backend.
// ---------------------------------------------------------------------------

/**
 * The reference `QueueStorage` backend: an array-backed FIFO, optionally
 * bounded. Serializes as a JSON array (element order = FIFO order) per
 * `lazily-spec/cell-model.md` § "Wire and snapshot shape".
 */
export class VecDequeStorage {
  /**
   * @param {{ elements?: unknown[], capacity?: number | null, closed?: boolean }} [initial]
   */
  constructor(initial = {}) {
    this.elements = Array.isArray(initial.elements) ? [...initial.elements] : [];
    this.#capacity =
      initial.capacity === undefined || initial.capacity === null
        ? null
        : initial.capacity;
    this.#closed = Boolean(initial.closed);
    if (this.#capacity !== null && this.#capacity <= 0) {
      throw new RangeError("VecDequeStorage capacity must be > 0");
    }
    Object.freeze(this);
  }

  #capacity;
  #closed;

  static from(initial) {
    return new VecDequeStorage(initial);
  }

  /**
   * Append `value` to the tail.
   * @returns {null | "Full" | "Closed"} `null` on success, else the error label.
   */
  tryPush(value) {
    if (this.#closed) {
      return QueuePushError.Closed;
    }
    if (this.#capacity !== null && this.elements.length >= this.#capacity) {
      return QueuePushError.Full;
    }
    this.elements.push(value);
    return null;
  }

  /**
   * Remove and return the head element.
   * @returns {unknown | "Empty" | "Closed"} the element, or the error label.
   */
  tryPop() {
    if (this.elements.length === 0) {
      return this.#closed ? QueuePopError.Closed : QueuePopError.Empty;
    }
    return this.elements.shift();
  }

  /** @returns {unknown} the head element, or `null` when empty. */
  peek() {
    return this.elements.length === 0 ? null : this.elements[0];
  }

  /** @returns {number} */
  len() {
    return this.elements.length;
  }

  /** @returns {number | null} the bounded capacity, or `null` if unbounded. */
  capacity() {
    return this.#capacity;
  }

  /** @returns {boolean} */
  isClosed() {
    return this.#closed;
  }

  /** Close the queue. Idempotent and terminal. */
  close() {
    this.#closed = true;
  }

  /** @returns {{ elements: unknown[], capacity: number | null, closed: boolean }} */
  snapshot() {
    return {
      elements: [...this.elements],
      capacity: this.#capacity,
      closed: this.#closed,
    };
  }
}

// ---------------------------------------------------------------------------
// QueueCell — the reactive shell (pure logic).
// ---------------------------------------------------------------------------

/**
 * A reactive FIFO queue — SPSC primitive with an MPSC usage rule (`#lzqueue`).
 *
 * Pure logic: wraps a pluggable `QueueStorage` backend and, after each op,
 * reports which reader kinds (`head` / `len` / `is_empty` / `is_full` /
 * `closed`) changed via the returned `invalidates` matrix. Wire the matrix to a
 * reactive `Context` to make the queue live-reactive. The reader-kind
 * independence law — a push to a non-empty queue does NOT invalidate the `head`
 * reader, a pop does — falls out of the transition predicates this shell
 * computes (derivable kinds by value-diff; `head` by op direction, so no `peek`
 * is required). `peek`/`capacity` are optional storage capabilities.
 */
export class QueueCell {
  /**
   * @param {{ elements?: unknown[], capacity?: number | null, closed?: boolean }} [initial]
   *   Passed to the default `VecDequeStorage` when no `storage` is given.
   * @param {object} [storage] A duck-typed `QueueStorage` backend. Defaults to
   *   a `VecDequeStorage` built from `initial`.
   */
  constructor(initial = {}, storage) {
    this.#storage = storage ?? new VecDequeStorage(initial);
    this.#prev = this.#snapshot();
    Object.freeze(this);
  }

  #storage;
  #prev;

  static from(initial, storage) {
    return new QueueCell(initial, storage);
  }

  // -- internal: reader-kind state + invalidation diff ----------------------

  #snapshot() {
    const len = this.#storage.len();
    const cap = this.#storage.capacity?.() ?? null;
    return {
      len,
      is_empty: len === 0,
      is_full: cap !== null && len >= cap,
      closed: this.#storage.isClosed(),
    };
  }

  /**
   * Diff the derivable reader-kinds (len / is_empty / is_full / closed) against
   * the previous snapshot. `head` is NOT derived here — it depends on op
   * *direction*, not just `len`, and deriving it would require `peek()`, which is
   * now an optional storage capability (`relaycell-backpressure-analysis.md`
   * §4.1). The caller passes `headChanged` from the transition predicate: a pop
   * always changes head; a push changes it only from empty. This keeps the
   * minimal storage contract (`tryPush`/`tryPop`/`len`/`isClosed`/`close`) free
   * of `peek`.
   * @param {boolean} headChanged
   */
  #diff(next, headChanged) {
    const prev = this.#prev;
    const invalidates = {
      head: headChanged,
      len: prev.len !== next.len,
      is_empty: prev.is_empty !== next.is_empty,
      is_full: prev.is_full !== next.is_full,
      closed: prev.closed !== next.closed,
    };
    this.#prev = next;
    return invalidates;
  }

  // -- mutating ops ---------------------------------------------------------

  /**
   * Append `value` to the tail.
   * @returns {{ returns: null | "Full" | "Closed", invalidates: QueueInvalidates }}
   *   On rejection (`Full` / `Closed`) the queue state is unchanged and the
   *   `invalidates` matrix is all-false.
   */
  tryPush(value) {
    const lenBefore = this.#storage.len();
    const err = this.#storage.tryPush(value);
    if (err !== null) {
      return { returns: err, invalidates: emptyInvalidates() };
    }
    // Head changes on a push only when the queue was empty (the new element
    // becomes the head); a push to a non-empty queue leaves head untouched —
    // the reader-kind independence law.
    const headChanged = lenBefore === 0;
    return { returns: null, invalidates: this.#diff(this.#snapshot(), headChanged) };
  }

  /**
   * Remove and return the head element. Pop on a closed *non-empty* queue
   * drains (returns the next element); only closed+empty yields `Closed`.
   * @returns {{ returns: unknown | "Empty" | "Closed", invalidates: QueueInvalidates }}
   */
  tryPop() {
    const value = this.#storage.tryPop();
    if (value === QueuePopError.Empty || value === QueuePopError.Closed) {
      return { returns: value, invalidates: emptyInvalidates() };
    }
    // A successful pop always advances the head (to the next element or empty).
    return { returns: value, invalidates: this.#diff(this.#snapshot(), true) };
  }

  /**
   * Close the queue. Idempotent (no-op on an already-closed queue) and
   * terminal. After close, `tryPush` returns `Closed`; `tryPop` drains and
   * returns `Closed` only once empty.
   * @returns {{ returns: null, invalidates: QueueInvalidates }}
   *   The `closed` reader is invalidated only on the open → closed transition.
   */
  close() {
    if (this.#storage.isClosed()) {
      return { returns: null, invalidates: emptyInvalidates() };
    }
    this.#storage.close();
    // Close touches only `closed`; head is unchanged.
    return { returns: null, invalidates: this.#diff(this.#snapshot(), false) };
  }

  // -- reader-kind reads (current state, non-mutating) ----------------------

  /**
   * Current head value, or `null` when empty. `peek` is an optional storage
   * capability: a backend without it (a raw channel) has no `head` reader, so
   * this returns `null` — exactly as an unbounded backend's `isFull` is always
   * `false`.
   */
  head() {
    return this.#storage.peek?.() ?? null;
  }

  /** Number of buffered elements. */
  len() {
    return this.#storage.len();
  }

  /** Whether the queue is empty. */
  isEmpty() {
    return this.#storage.len() === 0;
  }

  /**
   * Whether the queue is at capacity (the backpressure signal). Always `false`
   * for an unbounded backend.
   */
  isFull() {
    const cap = this.#storage.capacity?.() ?? null;
    return cap !== null && this.#storage.len() >= cap;
  }

  /** Whether the queue has been closed. */
  isClosed() {
    return this.#storage.isClosed();
  }

  /** The backend's capacity, or `null` if unbounded. */
  capacity() {
    return this.#storage.capacity?.() ?? null;
  }

  /**
   * Snapshot the buffered elements in FIFO order. There is no reactive
   * random-access `queue[N]` reader; per-position reactivity is the domain of
   * `CellMap`, not `QueueCell`.
   * @returns {unknown[]}
   */
  elements() {
    if (typeof this.#storage.elements === "function") {
      return this.#storage.elements();
    }
    return this.#storage.snapshot().elements;
  }
}

// ---------------------------------------------------------------------------
// TopicCell — broadcast log with independent subscriber cursors (#lztopiccell)
// ---------------------------------------------------------------------------

export const TopicDurability = Object.freeze({
  Durable: "durable",
  Ephemeral: "ephemeral",
});

export const TopicSubscribeOutcome = Object.freeze({
  Created: "Created",
  Reconnected: "Reconnected",
  AlreadyConnected: "AlreadyConnected",
});

/**
 * Broadcast topic: every subscriber receives every published element using an
 * independent, non-destructive cursor. Mutating operations return the exact
 * per-subscriber invalidation matrix used by the canonical fixtures.
 */
export class TopicCell {
  /**
   * @param {{
   *   base_offset?: number,
   *   elements?: unknown[],
   *   subscriptions?: Record<string, {cursor: number, durability: "durable" | "ephemeral", connected: boolean}>
   * }} [initial]
   */
  constructor(initial = {}) {
    this.#baseOffset = initial.base_offset ?? 0;
    this.#elements = Array.from(initial.elements ?? []);
    this.#subscriptions = new Map();
    if (!Number.isSafeInteger(this.#baseOffset) || this.#baseOffset < 0) {
      throw new RangeError("TopicCell base_offset must be a non-negative safe integer");
    }
    const end = this.endOffset();
    for (const [id, raw] of Object.entries(initial.subscriptions ?? {})) {
      if (!Number.isSafeInteger(raw.cursor) || raw.cursor < this.#baseOffset || raw.cursor > end) {
        throw new RangeError(`TopicCell cursor for ${id} is outside the retained offset range`);
      }
      if (raw.durability !== TopicDurability.Durable && raw.durability !== TopicDurability.Ephemeral) {
        throw new TypeError(`invalid TopicCell durability for ${id}`);
      }
      if (typeof raw.connected !== "boolean") {
        throw new TypeError(`TopicCell connected flag for ${id} must be boolean`);
      }
      if (raw.durability === TopicDurability.Ephemeral && !raw.connected) {
        throw new TypeError(`disconnected ephemeral TopicCell subscription ${id} must be removed`);
      }
      this.#subscriptions.set(id, {
        cursor: raw.cursor,
        durability: raw.durability,
        connected: raw.connected,
      });
    }
  }

  #baseOffset;
  #elements;
  #subscriptions;

  static from(initial = {}) {
    return new TopicCell(initial);
  }

  #allFalse() {
    return Object.fromEntries(Array.from(this.#subscriptions.keys(), (id) => [id, false]));
  }

  #only(id, changed) {
    const invalidates = this.#allFalse();
    invalidates[id] = changed;
    return invalidates;
  }

  /** Create at tail, or reconnect an existing durable identity in place. */
  subscribe(id, durability) {
    const existing = this.#subscriptions.get(id);
    if (existing) {
      if (existing.connected) {
        return {
          returns: TopicSubscribeOutcome.AlreadyConnected,
          invalidates: this.#allFalse(),
        };
      }
      existing.connected = true;
      return {
        returns: TopicSubscribeOutcome.Reconnected,
        invalidates: this.#only(id, true),
      };
    }
    if (durability !== TopicDurability.Durable && durability !== TopicDurability.Ephemeral) {
      throw new TypeError(`invalid TopicCell durability for ${id}`);
    }
    this.#subscriptions.set(id, {
      cursor: this.endOffset(),
      durability,
      connected: true,
    });
    return { returns: TopicSubscribeOutcome.Created, invalidates: this.#only(id, true) };
  }

  /** Reconnect a durable identity; unknown ids are created at the current tail. */
  reconnect(id) {
    return this.subscribe(id, TopicDurability.Durable);
  }

  /** Durable ids remain offline; ephemeral ids are removed. */
  disconnect(id) {
    const sub = this.#subscriptions.get(id);
    if (!sub || !sub.connected) {
      return { returns: null, invalidates: this.#allFalse() };
    }
    const invalidates = this.#only(id, true);
    if (sub.durability === TopicDurability.Ephemeral) {
      this.#subscriptions.delete(id);
    } else {
      sub.connected = false;
    }
    return { returns: null, invalidates };
  }

  /** Append one element without moving any cursor. */
  publish(value) {
    const offset = this.endOffset();
    this.#elements.push(value);
    const invalidates = Object.fromEntries(
      Array.from(this.#subscriptions, ([id, sub]) => [
        id,
        sub.connected && sub.cursor <= offset,
      ]),
    );
    return { returns: null, offset, invalidates };
  }

  /** Unread suffix for a connected subscriber. */
  readStream(id) {
    const sub = this.#subscriptions.get(id);
    if (!sub || !sub.connected) return [];
    return this.#elements.slice(Math.max(0, sub.cursor - this.#baseOffset));
  }

  /** Element at a subscriber cursor, or null at the tail/offline. */
  read(id) {
    return this.readStream(id)[0] ?? null;
  }

  /** Advance only the named connected cursor by one. */
  advance(id) {
    const sub = this.#subscriptions.get(id);
    if (!sub || !sub.connected || sub.cursor >= this.endOffset()) {
      return { returns: null, invalidates: this.#allFalse() };
    }
    const value = this.#elements[sub.cursor - this.#baseOffset];
    sub.cursor += 1;
    return { returns: value, invalidates: this.#only(id, true) };
  }

  /** Process restart is observational: persisted durable state is unchanged. */
  restart(_id) {
    return { returns: null, invalidates: this.#allFalse() };
  }

  /** Remove only the prefix below the minimum durable absolute cursor. */
  gc() {
    const durable = Array.from(this.#subscriptions.values())
      .filter((sub) => sub.durability === TopicDurability.Durable)
      .map((sub) => sub.cursor);
    const frontier = durable.length === 0 ? this.endOffset() : Math.min(...durable);
    const removed = frontier - this.#baseOffset;
    this.#elements.splice(0, removed);
    this.#baseOffset = frontier;
    return { returns: removed, invalidates: this.#allFalse() };
  }

  baseOffset() {
    return this.#baseOffset;
  }

  endOffset() {
    return this.#baseOffset + this.#elements.length;
  }

  elements() {
    return Array.from(this.#elements);
  }

  subscription(id) {
    const sub = this.#subscriptions.get(id);
    return sub ? { ...sub } : null;
  }

  subscriptions() {
    return Object.fromEntries(Array.from(this.#subscriptions, ([id, sub]) => [id, { ...sub }]));
  }

  snapshot() {
    return {
      base_offset: this.#baseOffset,
      elements: this.elements(),
      subscriptions: this.subscriptions(),
    };
  }
}

// ---------------------------------------------------------------------------
// WorkQueueCell — competing consumers with exclusive leases (#lzworkqueue)
// ---------------------------------------------------------------------------

export const WorkQueueDeadLetterReason = Object.freeze({
  Nack: "nack",
  Expired: "expired",
});

/**
 * Pull-based competing-consumer work queue.
 *
 * This is the portable local-authority lifecycle. The owning instance
 * serializes `claim`; a distributed/HA host puts that decision behind its
 * leader or consensus log while preserving the same operation outcomes.
 */
export class WorkQueueCell {
  /** @param {{visibility_timeout: number, max_deliveries: number}} config */
  constructor(config) {
    if (!config || !Number.isSafeInteger(config.visibility_timeout) || config.visibility_timeout <= 0) {
      throw new RangeError("visibility_timeout must be a positive safe integer");
    }
    if (!Number.isSafeInteger(config.max_deliveries) || config.max_deliveries < 1) {
      throw new RangeError("max_deliveries must be at least one");
    }
    this.#visibilityTimeout = config.visibility_timeout;
    this.#maxDeliveries = config.max_deliveries;
  }

  #visibilityTimeout;
  #maxDeliveries;
  #pending = [];
  #inFlight = new Map();
  #deadLetters = [];
  #nextItemId = 0;
  #nextDeliveryId = 0;

  #counts() {
    return {
      pending: this.#pending.length,
      in_flight: this.#inFlight.size,
      dead_letters: this.#deadLetters.length,
    };
  }

  #invalidates(before) {
    const after = this.#counts();
    return {
      pending_len: before.pending !== after.pending,
      is_empty: (before.pending === 0) !== (after.pending === 0),
      in_flight_len: before.in_flight !== after.in_flight,
      dead_letter_len: before.dead_letters !== after.dead_letters,
    };
  }

  #unchanged() {
    return {
      pending_len: false,
      is_empty: false,
      in_flight_len: false,
      dead_letter_len: false,
    };
  }

  #validateNow(now) {
    if (!Number.isSafeInteger(now) || now < 0) {
      throw new RangeError("now must be a non-negative safe integer");
    }
  }

  #fail(delivery, reason) {
    if (delivery.attempt < this.#maxDeliveries) {
      this.#pending.push({
        item_id: delivery.item_id,
        value: delivery.value,
        attempts: delivery.attempt,
      });
    } else {
      this.#deadLetters.push({
        item_id: delivery.item_id,
        value: delivery.value,
        attempts: delivery.attempt,
        reason,
      });
    }
  }

  /** Append a pending item and return its stable item id. */
  push(value) {
    const before = this.#counts();
    if (this.#nextItemId >= Number.MAX_SAFE_INTEGER) throw new RangeError("item id exhausted");
    const itemId = this.#nextItemId;
    this.#nextItemId += 1;
    this.#pending.push({ item_id: itemId, value, attempts: 0 });
    return { returns: itemId, invalidates: this.#invalidates(before) };
  }

  /** Claim the oldest pending item for one worker. */
  claim(worker, now) {
    this.#validateNow(now);
    if (this.#pending.length === 0) {
      return { returns: null, invalidates: this.#unchanged() };
    }
    if (this.#nextDeliveryId >= Number.MAX_SAFE_INTEGER) throw new RangeError("delivery id exhausted");
    const before = this.#counts();
    const item = this.#pending.shift();
    const deliveryId = this.#nextDeliveryId;
    this.#nextDeliveryId += 1;
    const delivery = {
      delivery_id: deliveryId,
      item_id: item.item_id,
      value: item.value,
      worker,
      attempt: item.attempts + 1,
      deadline: Math.min(Number.MAX_SAFE_INTEGER, now + this.#visibilityTimeout),
    };
    this.#inFlight.set(deliveryId, delivery);
    return { returns: { ...delivery }, invalidates: this.#invalidates(before) };
  }

  /** Ack only the exact current delivery owned by `worker`. */
  ack(worker, deliveryId) {
    const delivery = this.#inFlight.get(deliveryId);
    if (!delivery || delivery.worker !== worker) {
      return { returns: false, invalidates: this.#unchanged() };
    }
    const before = this.#counts();
    this.#inFlight.delete(deliveryId);
    return { returns: true, invalidates: this.#invalidates(before) };
  }

  /** Nack a delivery, requeueing at the tail or dead-lettering at the limit. */
  nack(worker, deliveryId) {
    const delivery = this.#inFlight.get(deliveryId);
    if (!delivery || delivery.worker !== worker) {
      return { returns: false, invalidates: this.#unchanged() };
    }
    const before = this.#counts();
    this.#inFlight.delete(deliveryId);
    this.#fail(delivery, WorkQueueDeadLetterReason.Nack);
    return { returns: true, invalidates: this.#invalidates(before) };
  }

  /** Expire every lease with `deadline < now`, in delivery-id order. */
  reapExpired(now) {
    this.#validateNow(now);
    const expired = Array.from(this.#inFlight.values())
      .filter((delivery) => delivery.deadline < now)
      .sort((a, b) => a.delivery_id - b.delivery_id);
    if (expired.length === 0) {
      return { returns: 0, invalidates: this.#unchanged() };
    }
    const before = this.#counts();
    for (const delivery of expired) {
      this.#inFlight.delete(delivery.delivery_id);
      this.#fail(delivery, WorkQueueDeadLetterReason.Expired);
    }
    return { returns: expired.length, invalidates: this.#invalidates(before) };
  }

  pendingLen() {
    return this.#pending.length;
  }

  isEmpty() {
    return this.#pending.length === 0;
  }

  inFlightLen() {
    return this.#inFlight.size;
  }

  deadLetterLen() {
    return this.#deadLetters.length;
  }

  pendingItems() {
    return this.#pending.map((item) => ({ ...item }));
  }

  inFlightDeliveries() {
    return Array.from(this.#inFlight.values(), (delivery) => ({ ...delivery })).sort(
      (a, b) => a.delivery_id - b.delivery_id,
    );
  }

  deadLetterItems() {
    return this.#deadLetters.map((item) => ({ ...item }));
  }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/** @returns {QueueInvalidates} */
function emptyInvalidates() {
  return { head: false, len: false, is_empty: false, is_full: false, closed: false };
}
