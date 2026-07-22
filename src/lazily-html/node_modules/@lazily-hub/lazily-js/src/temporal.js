// Temporal source primitives (#lztime) — the JS port.
//
// See `lazily-spec/docs/temporal-sources.md` and the formal model
// `lazily-formal/LazilyFormal/Temporal.lean`. Time is a monotone **logical
// clock** (`now`, an integer) exactly like the relay policies; a binding drives
// the sources from its own runtime timer by feeding a non-decreasing `now`.
//
// Each source is a pure compute **core** (`TimerCore`/`IntervalCore`/`CronCore`/
// `DeadlineCore`) — the C++/bytes-eligible part — split from a thin reactive
// **cell** that projects the core's fire edge onto a `Context` cell so
// dependents invalidate *only on an actual fire* (the backend-portability rule).

// ---------------------------------------------------------------------------
// Single-shot timer
// ---------------------------------------------------------------------------

/** Single-shot core: fires exactly once at the first tick with `now >= fireAt`. */
export class TimerCore {
  constructor(fireAt) {
    this.fireAt = fireAt;
    this.fired = false;
  }
  /** Advance to `now`; returns the fire edge (true only on the first fire). */
  tick(now) {
    if (this.fired || now < this.fireAt) return false;
    this.fired = true;
    return true;
  }
  nextFire() {
    return this.fired ? null : this.fireAt;
  }
}

/** Reactive single-shot timer: edge-only invalidation of `fired`/`value`. */
export class TimerCell {
  constructor(ctx, fireAt) {
    this.ctx = ctx;
    this.core = new TimerCore(fireAt);
    this.firedCell = ctx.source(false);
  }
  tick(now) {
    const edge = this.core.tick(now);
    if (edge) this.ctx.set(this.firedCell, true);
    return edge;
  }
  hasFired() {
    return this.ctx.get(this.firedCell);
  }
  /** `null` before the fire, the unit marker (`true`) after. */
  value() {
    return this.ctx.get(this.firedCell) ? true : null;
  }
  nextFire() {
    return this.core.nextFire();
  }
}

// ---------------------------------------------------------------------------
// Periodic interval
// ---------------------------------------------------------------------------

/** Periodic core: boundaries at `period, 2*period, ...`; a tick counts every
 *  boundary in `(frontier, now]`, so a jump past several counts them all. */
export class IntervalCore {
  constructor(period) {
    this.period = period < 1 ? 1 : period;
    this.next = this.period;
    this.count = 0;
  }
  #firesThisTick(now) {
    return now < this.next ? 0 : Math.floor((now - this.next) / this.period) + 1;
  }
  tick(now) {
    const fires = this.#firesThisTick(now);
    if (fires === 0) return false;
    this.count += fires;
    this.next += fires * this.period;
    return true;
  }
  nextFire() {
    return this.next;
  }
}

/** Reactive periodic interval: invalidates only when `count` changes. */
export class IntervalCell {
  constructor(ctx, period) {
    this.ctx = ctx;
    this.core = new IntervalCore(period);
    this.countCell = ctx.source(0);
  }
  tick(now) {
    const edge = this.core.tick(now);
    if (edge) this.ctx.set(this.countCell, this.core.count);
    return edge;
  }
  count() {
    return this.ctx.get(this.countCell);
  }
  nextFire() {
    return this.core.nextFire();
  }
}

// ---------------------------------------------------------------------------
// Cron pattern
// ---------------------------------------------------------------------------

/** Count of `m in 1..=n` with `m mod cycle === o` (`0 <= o < cycle`). */
function countUpto(n, o, cycle) {
  if (o === 0) return Math.floor(n / cycle);
  if (o <= n) return Math.floor((n - o) / cycle) + 1;
  return 0;
}

/** Pattern-periodic core: a tick `m >= 1` fires iff `m mod cycle` is in
 *  `offsets` — an interval with a match set (a cron expression's shape). */
export class CronCore {
  constructor(cycle, offsets) {
    this.cycle = cycle < 1 ? 1 : cycle;
    this.offsets = [...new Set(offsets.map((o) => ((o % this.cycle) + this.cycle) % this.cycle))].sort(
      (a, b) => a - b,
    );
    this.cursor = 0;
    this.count = 0;
  }
  #matchesIn(lo, hi) {
    let sum = 0;
    for (const o of this.offsets) sum += countUpto(hi, o, this.cycle) - countUpto(lo, o, this.cycle);
    return sum;
  }
  tick(now) {
    if (now <= this.cursor) {
      this.cursor = Math.max(this.cursor, now);
      return false;
    }
    const fires = this.#matchesIn(this.cursor, now);
    this.cursor = now;
    if (fires === 0) return false;
    this.count += fires;
    return true;
  }
  nextFire() {
    if (this.offsets.length === 0) return null;
    const start = this.cursor + 1;
    const base = Math.floor(start / this.cycle) * this.cycle;
    for (let cyc = 0; cyc < 2; cyc++) {
      const block = base + cyc * this.cycle;
      for (const o of this.offsets) {
        const cand = block + o;
        if (cand >= start) return cand;
      }
    }
    return null;
  }
}

/** Reactive cron source: same reactive contract as `IntervalCell`. */
export class CronCell {
  constructor(ctx, cycle, offsets) {
    this.ctx = ctx;
    this.core = new CronCore(cycle, offsets);
    this.countCell = ctx.source(0);
  }
  tick(now) {
    const edge = this.core.tick(now);
    if (edge) this.ctx.set(this.countCell, this.core.count);
    return edge;
  }
  count() {
    return this.ctx.get(this.countCell);
  }
  nextFire() {
    return this.core.nextFire();
  }
}

// ---------------------------------------------------------------------------
// Value + deadline
// ---------------------------------------------------------------------------

/** Liveness state label for a `DeadlineCell`. */
export const DeadlinedState = Object.freeze({ Live: "Live", Expired: "Expired" });

/** Deadline core (bytes-eligible): a `TimerCore` over the deadline. */
export class DeadlineCore {
  constructor(deadline) {
    this.timer = new TimerCore(deadline);
  }
  get isExpired() {
    return this.timer.fired;
  }
  tick(now) {
    return this.timer.tick(now);
  }
  nextFire() {
    return this.timer.nextFire();
  }
}

/** Reactive value + deadline: flips `Live(v) -> Expired(v)` at the deadline,
 *  preserving the value; `state` invalidates only on the expiry edge. */
export class DeadlineCell {
  constructor(ctx, value, deadline) {
    this.ctx = ctx;
    this.value = value;
    this.core = new DeadlineCore(deadline);
    this.expiredCell = ctx.source(false);
  }
  tick(now) {
    const edge = this.core.tick(now);
    if (edge) this.ctx.set(this.expiredCell, true);
    return edge;
  }
  /** `{ state: "Live"|"Expired", value }` — the value is preserved across the flip. */
  state() {
    const expired = this.ctx.get(this.expiredCell);
    return { state: expired ? DeadlinedState.Expired : DeadlinedState.Live, value: this.value };
  }
  isExpired() {
    return this.ctx.get(this.expiredCell);
  }
  nextFire() {
    return this.core.nextFire();
  }
}
