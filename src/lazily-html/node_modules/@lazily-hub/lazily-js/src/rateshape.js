// Rate-shaping source operators (#lzrateshape) — the JS port.
//
// See `lazily-spec/docs/rate-shaping.md` and the formal model
// `lazily-formal/LazilyFormal/RateShape.lean`. Lifts debounce / throttle /
// time-sampling out of the relay egress so any `Reactive<T>` source can be
// rate-shaped. Each operator is a pure compute **core** (the emit/drop decision)
// split from a thin reactive **cell** projecting the emitted value onto a
// `Context` cell so a dropped input never invalidates dependents. Time is the
// same monotone logical clock as `#lztime`. An emitted value of `null` from an
// op means "nothing emitted this op".

// ---------------------------------------------------------------------------
// Debounce
// ---------------------------------------------------------------------------

/** Debounce core: coalesce inputs (KeepLatest) and emit the latest value only
 *  after `quiet` ticks with no new input; every input resets the deadline. */
export class DebounceCore {
  constructor(quiet) {
    this.quiet = quiet;
    this.pending = null;
    this.hasPending = false;
    this.fireAt = 0;
    this.armed = false;
  }
  input(now, v) {
    this.pending = v;
    this.hasPending = true;
    this.fireAt = now + this.quiet;
    this.armed = true;
  }
  tick(now) {
    if (this.armed && this.hasPending && this.fireAt <= now) {
      this.armed = false;
      this.hasPending = false;
      const p = this.pending;
      this.pending = null;
      return p;
    }
    return null;
  }
}

/** Reactive debounce over any `Reactive<T>` source. */
export class DebounceCell {
  constructor(ctx, quiet) {
    this.ctx = ctx;
    this.core = new DebounceCore(quiet);
    this.outputCell = ctx.source(null);
  }
  input(now, v) {
    this.core.input(now, v);
  }
  tick(now) {
    const emitted = this.core.tick(now);
    if (emitted !== null) this.ctx.set(this.outputCell, emitted);
    return emitted;
  }
  output() {
    return this.ctx.get(this.outputCell);
  }
}

// ---------------------------------------------------------------------------
// Throttle
// ---------------------------------------------------------------------------

export const ThrottleEdge = Object.freeze({ Leading: "Leading", Trailing: "Trailing" });

/** Throttle core: at most one emit per `window`. */
export class ThrottleCore {
  constructor(edge, window) {
    this.edge = edge;
    this.window = window;
    this.windowEnd = null;
    this.windowStart = null;
    this.pending = null;
    this.hasPending = false;
  }
  input(now, v) {
    if (this.edge === ThrottleEdge.Leading) {
      if (this.windowEnd !== null && now < this.windowEnd) return null;
      this.windowEnd = now + this.window;
      return v;
    }
    // Trailing
    if (this.windowStart === null) this.windowStart = now;
    this.pending = v;
    this.hasPending = true;
    return null;
  }
  tick(now) {
    if (this.edge !== ThrottleEdge.Trailing) return null;
    if (this.windowStart === null) return null;
    if (now >= this.windowStart + this.window && this.hasPending) {
      this.windowStart = null;
      this.hasPending = false;
      const p = this.pending;
      this.pending = null;
      return p;
    }
    return null;
  }
}

/** Reactive throttle over any `Reactive<T>` source. */
export class ThrottleCell {
  constructor(ctx, edge, window) {
    this.ctx = ctx;
    this.core = new ThrottleCore(edge, window);
    this.outputCell = ctx.source(null);
  }
  input(now, v) {
    const emitted = this.core.input(now, v);
    if (emitted !== null) this.ctx.set(this.outputCell, emitted);
    return emitted;
  }
  tick(now) {
    const emitted = this.core.tick(now);
    if (emitted !== null) this.ctx.set(this.outputCell, emitted);
    return emitted;
  }
  output() {
    return this.ctx.get(this.outputCell);
  }
}

// ---------------------------------------------------------------------------
// Sample
// ---------------------------------------------------------------------------

export const SampleMode = Object.freeze({
  count: (n) => ({ kind: "Count", n }),
  time: (period) => ({ kind: "Time", period }),
});

/** Deterministic sampling core. */
export class SampleCore {
  constructor(mode) {
    this.mode = mode;
    this.counter = 0;
    this.next = mode.kind === "Time" ? Math.max(1, mode.period) : 0;
    this.held = null;
  }
  input(v) {
    if (this.mode.kind === "Count") {
      const n = Math.max(1, this.mode.n);
      this.counter += 1;
      return this.counter % n === 0 ? v : null;
    }
    // Time: hold the latest.
    this.held = v;
    return null;
  }
  tick(now) {
    if (this.mode.kind !== "Time") return null;
    const period = Math.max(1, this.mode.period);
    if (now < this.next) return null;
    const fires = Math.floor((now - this.next) / period) + 1;
    this.next += fires * period;
    return this.held; // held latest persists across emits
  }
}

/** Reactive sampler over any `Reactive<T>` source. */
export class SampleCell {
  constructor(ctx, mode) {
    this.ctx = ctx;
    this.core = new SampleCore(mode);
    this.outputCell = ctx.source(null);
  }
  input(v) {
    const emitted = this.core.input(v);
    if (emitted !== null) this.ctx.set(this.outputCell, emitted);
    return emitted;
  }
  tick(now) {
    const emitted = this.core.tick(now);
    if (emitted !== null) this.ctx.set(this.outputCell, emitted);
    return emitted;
  }
  output() {
    return this.ctx.get(this.outputCell);
  }
}

// ---------------------------------------------------------------------------
// Probabilistic sample
// ---------------------------------------------------------------------------

/** A small deterministic SplitMix64 RNG — `nextDouble()` yields a draw in [0, 1). */
export class Lcg {
  constructor(seed) {
    this.state = BigInt.asUintN(64, BigInt(seed));
  }
  nextDouble() {
    const MASK = (1n << 64n) - 1n;
    this.state = (this.state + 0x9e3779b97f4a7c15n) & MASK;
    let z = this.state;
    z = ((z ^ (z >> 30n)) * 0xbf58476d1ce4e5b9n) & MASK;
    z = ((z ^ (z >> 27n)) * 0x94d049bb133111ebn) & MASK;
    z = z ^ (z >> 31n);
    return Number(z >> 11n) / Number(1n << 53n);
  }
}

/** Probabilistic (tail) sampling core — a draw in [0,1) passes iff draw < rate. */
export class ProbabilisticSampleCore {
  constructor(rate) {
    this.rate = Math.min(1, Math.max(0, rate));
  }
  decide(draw) {
    return draw < this.rate;
  }
}

/** Reactive probabilistic sampler; owns an injectable RNG (`{ nextDouble() }`). */
export class ProbabilisticSampleCell {
  constructor(ctx, rate, rng) {
    this.ctx = ctx;
    this.core = new ProbabilisticSampleCore(rate);
    this.rng = rng;
    this.outputCell = ctx.source(null);
  }
  input(v) {
    return this.inputWithDraw(v, this.rng.nextDouble());
  }
  inputWithDraw(v, draw) {
    if (this.core.decide(draw)) {
      this.ctx.set(this.outputCell, v);
      return v;
    }
    return null;
  }
  output() {
    return this.ctx.get(this.outputCell);
  }
}
