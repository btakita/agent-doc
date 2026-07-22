// Stream windowing (#lzwindow) — the JS port.
//
// See `lazily-spec/docs/windowing.md` and the formal model
// `lazily-formal/LazilyFormal/Windowing.lean`. Window aggregation *is* a merge:
// the aggregate of a window equals the associative fold of its elements. The
// cores take a `merge(a, b)` fold (e.g. Sum = `(a, b) => a + b`). Each core is
// split from a reactive cell projecting the last emitted aggregate. An emitted
// value of `null` means "nothing emitted".

function foldWindow(items, merge) {
  if (items.length === 0) return null;
  return items.reduce((acc, x) => merge(acc, x));
}

// ---------------------------------------------------------------------------
// Cores
// ---------------------------------------------------------------------------

export class TumblingCountCore {
  constructor(n, merge) {
    this.n = Math.max(1, n);
    this.merge = merge;
    this.acc = null;
    this.count = 0;
  }
  push(v) {
    this.acc = this.acc === null ? v : this.merge(this.acc, v);
    this.count += 1;
    if (this.count >= this.n) {
      this.count = 0;
      const e = this.acc;
      this.acc = null;
      return e;
    }
    return null;
  }
}

export class TumblingTimeCore {
  constructor(period, merge) {
    this.period = Math.max(1, period);
    this.next = this.period;
    this.merge = merge;
    this.acc = null;
  }
  push(_now, v) {
    this.acc = this.acc === null ? v : this.merge(this.acc, v);
  }
  tick(now) {
    if (now < this.next) return null;
    while (this.next <= now) this.next += this.period;
    const e = this.acc;
    this.acc = null;
    return e;
  }
}

export class SlidingCore {
  constructor(size, slide, merge) {
    this.size = Math.max(1, size);
    this.slide = Math.max(1, slide);
    this.merge = merge;
    this.buffer = [];
    this.since = 0;
  }
  push(v) {
    this.buffer.push(v);
    while (this.buffer.length > this.size) this.buffer.shift();
    this.since += 1;
    if (this.since >= this.slide) {
      this.since = 0;
      return foldWindow(this.buffer, this.merge);
    }
    return null;
  }
}

export class SessionCore {
  constructor(gap, merge) {
    this.gap = gap;
    this.merge = merge;
    this.acc = null;
    this.last = null;
  }
  push(now, v) {
    const idleBreak = this.last !== null && now - this.last > this.gap && this.acc !== null;
    if (idleBreak) {
      const emit = this.acc;
      this.acc = v;
      this.last = now;
      return emit;
    }
    this.acc = this.acc === null ? v : this.merge(this.acc, v);
    this.last = now;
    return null;
  }
  flush(now) {
    if (this.last !== null && now - this.last > this.gap && this.acc !== null) {
      const emit = this.acc;
      this.acc = null;
      return emit;
    }
    return null;
  }
}

// ---------------------------------------------------------------------------
// Reactive cells
// ---------------------------------------------------------------------------

class WindowOutput {
  constructor(ctx) {
    this.ctx = ctx;
    this.outputCell = ctx.source(null);
  }
  emit(e) {
    if (e !== null) this.ctx.set(this.outputCell, e);
    return e;
  }
  value() {
    return this.ctx.get(this.outputCell);
  }
}

export class TumblingCountWindow {
  constructor(ctx, n, merge) {
    this.core = new TumblingCountCore(n, merge);
    this.out = new WindowOutput(ctx);
    this.outputCell = this.out.outputCell;
  }
  push(v) {
    return this.out.emit(this.core.push(v));
  }
  output() {
    return this.out.value();
  }
}

export class TumblingTimeWindow {
  constructor(ctx, period, merge) {
    this.core = new TumblingTimeCore(period, merge);
    this.out = new WindowOutput(ctx);
    this.outputCell = this.out.outputCell;
  }
  push(now, v) {
    this.core.push(now, v);
  }
  tick(now) {
    return this.out.emit(this.core.tick(now));
  }
  output() {
    return this.out.value();
  }
}

export class SlidingWindow {
  constructor(ctx, size, slide, merge) {
    this.core = new SlidingCore(size, slide, merge);
    this.out = new WindowOutput(ctx);
    this.outputCell = this.out.outputCell;
  }
  push(v) {
    return this.out.emit(this.core.push(v));
  }
  output() {
    return this.out.value();
  }
}

export class SessionWindow {
  constructor(ctx, gap, merge) {
    this.core = new SessionCore(gap, merge);
    this.out = new WindowOutput(ctx);
    this.outputCell = this.out.outputCell;
  }
  push(now, v) {
    return this.out.emit(this.core.push(now, v));
  }
  flush(now) {
    return this.out.emit(this.core.flush(now));
  }
  output() {
    return this.out.value();
  }
}
