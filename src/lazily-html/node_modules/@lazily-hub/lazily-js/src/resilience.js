// Fault-tolerance primitives (#lzresilience) — the JS port.
//
// See `lazily-spec/docs/resilience.md` and the formal model
// `lazily-formal/LazilyFormal/Resilience.lean`. Circuit breaker / retry /
// bulkhead / timeout, each a pure compute core split from a reactive cell
// projecting the salient reader. Composes with the command transport so RPCs
// degrade gracefully.

// ---------------------------------------------------------------------------
// Circuit breaker
// ---------------------------------------------------------------------------

export const BreakerState = Object.freeze({
  Closed: "Closed",
  Open: "Open",
  HalfOpen: "HalfOpen",
});

/** Circuit-breaker core: a sliding window of outcomes trips Closed->Open at the
 *  failure threshold; Open->HalfOpen at the deadline; a HalfOpen success closes. */
export class CircuitBreakerCore {
  constructor(window, failureThreshold, resetTimeout) {
    this.window = Math.max(1, window);
    this.failureThreshold = Math.max(1, failureThreshold);
    this.resetTimeout = resetTimeout;
    this.state = BreakerState.Closed;
    this.outcomes = []; // true = success
    this.openUntil = 0;
  }
  #failures() {
    return this.outcomes.filter((s) => !s).length;
  }
  allow(now) {
    if (this.state === BreakerState.Closed) return true;
    if (this.state === BreakerState.Open) {
      if (now >= this.openUntil) {
        this.state = BreakerState.HalfOpen;
        return true;
      }
      return false;
    }
    return true; // HalfOpen probe
  }
  record(success, now) {
    if (this.state === BreakerState.HalfOpen) {
      if (success) {
        this.state = BreakerState.Closed;
        this.outcomes = [];
      } else {
        this.state = BreakerState.Open;
        this.openUntil = now + this.resetTimeout;
      }
    } else if (this.state === BreakerState.Closed) {
      this.outcomes.push(success);
      while (this.outcomes.length > this.window) this.outcomes.shift();
      if (this.#failures() >= this.failureThreshold) {
        this.state = BreakerState.Open;
        this.openUntil = now + this.resetTimeout;
      }
    }
  }
}

/** Reactive circuit breaker: projects the state onto a cell. */
export class CircuitBreakerCell {
  constructor(ctx, window, failureThreshold, resetTimeout) {
    this.ctx = ctx;
    this.core = new CircuitBreakerCore(window, failureThreshold, resetTimeout);
    this.stateCell = ctx.source(BreakerState.Closed);
  }
  #refresh() {
    this.ctx.set(this.stateCell, this.core.state);
  }
  allow(now) {
    const r = this.core.allow(now);
    this.#refresh();
    return r;
  }
  record(success, now) {
    this.core.record(success, now);
    this.#refresh();
  }
  state() {
    return this.core.state;
  }
}

// ---------------------------------------------------------------------------
// Retry backoff
// ---------------------------------------------------------------------------

/** Exponential-backoff core: delay(attempt) = min(cap, base * 2^attempt),
 *  saturating to cap. */
export class RetryPolicyCore {
  constructor(base, cap) {
    this.base = base;
    this.cap = cap;
    this.attempt = 0;
  }
  delay(attempt) {
    if (attempt >= 63) return this.cap;
    return Math.min(this.cap, this.base * 2 ** attempt);
  }
  nextDelay() {
    const d = this.delay(this.attempt);
    this.attempt += 1;
    return d;
  }
  reset() {
    this.attempt = 0;
  }
}

/** Reactive retry policy: projects the current delay onto a cell. */
export class RetryPolicyCell {
  constructor(ctx, base, cap) {
    this.ctx = ctx;
    this.core = new RetryPolicyCore(base, cap);
    this.delayCell = ctx.source(0);
  }
  nextDelay() {
    const d = this.core.nextDelay();
    this.ctx.set(this.delayCell, d);
    return d;
  }
  reset() {
    this.core.reset();
    this.ctx.set(this.delayCell, 0);
  }
  delay() {
    return this.ctx.get(this.delayCell);
  }
}

// ---------------------------------------------------------------------------
// Bulkhead
// ---------------------------------------------------------------------------

/** Bounded isolation-pool core. */
export class BulkheadCore {
  constructor(capacity) {
    this.capacity = capacity;
    this.inUse = 0;
  }
  acquire() {
    if (this.inUse < this.capacity) {
      this.inUse += 1;
      return true;
    }
    return false;
  }
  release() {
    if (this.inUse > 0) this.inUse -= 1;
  }
}

/** Reactive bulkhead: projects permitsInUse onto a cell. */
export class BulkheadCell {
  constructor(ctx, capacity) {
    this.ctx = ctx;
    this.core = new BulkheadCore(capacity);
    this.inUseCell = ctx.source(0);
  }
  #refresh() {
    this.ctx.set(this.inUseCell, this.core.inUse);
  }
  acquire() {
    const r = this.core.acquire();
    this.#refresh();
    return r;
  }
  release() {
    this.core.release();
    this.#refresh();
  }
  permitsInUse() {
    return this.ctx.get(this.inUseCell);
  }
}

// ---------------------------------------------------------------------------
// Timeout
// ---------------------------------------------------------------------------

/** Deadline-bounded call core. */
export class TimeoutCore {
  constructor() {
    this.deadline = 0;
    this.armed = false;
    this.timedOut = false;
  }
  arm(now, timeout) {
    this.deadline = now + timeout;
    this.armed = true;
    this.timedOut = false;
  }
  tick(now) {
    if (this.armed && !this.timedOut && now >= this.deadline) {
      this.timedOut = true;
      return true;
    }
    return false;
  }
  isTimedOut() {
    return this.timedOut;
  }
}

/** Reactive timeout: projects isTimedOut onto a cell. */
export class TimeoutCell {
  constructor(ctx) {
    this.ctx = ctx;
    this.core = new TimeoutCore();
    this.timedOutCell = ctx.source(false);
  }
  #refresh() {
    this.ctx.set(this.timedOutCell, this.core.isTimedOut());
  }
  arm(now, timeout) {
    this.core.arm(now, timeout);
    this.#refresh();
  }
  tick(now) {
    const r = this.core.tick(now);
    this.#refresh();
    return r;
  }
  isTimedOut() {
    return this.ctx.get(this.timedOutCell);
  }
}
