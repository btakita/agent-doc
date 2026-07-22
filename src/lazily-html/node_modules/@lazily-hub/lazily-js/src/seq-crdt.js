// Move-aware sequence CRDT (cell-model.md § Move-aware sequence order,
// #lzseqcrdt). Each element is three independent LWW registers — value,
// position (fractional-index byte key + peer), deleted — each stamped by an
// HLC. A move is a SINGLE LWW reassignment of the position register (not
// delete + reinsert), so concurrent moves of the same element converge to the
// later stamp without duplication; a concurrent move + value-edit both apply
// (independent registers). Removal is an LWW tombstone. Order is the
// lexicographic total order on (frac, peer).

// Hybrid logical clock stamp: total order is (wall_time, logical, peer).
export class HlcStamp {
  constructor(wallTime, logical, peer) {
    this.wallTime = wallTime;
    this.logical = logical;
    this.peer = peer;
    Object.freeze(this);
  }
  compareTo(other) {
    if (this.wallTime !== other.wallTime) {
      return this.wallTime < other.wallTime ? -1 : 1;
    }
    if (this.logical !== other.logical) {
      return this.logical < other.logical ? -1 : 1;
    }
    if (this.peer !== other.peer) {
      return this.peer < other.peer ? -1 : 1;
    }
    return 0;
  }
  static max(a, b) {
    return a.compareTo(b) >= 0 ? a : b;
  }
}

// HLC: guarantees strictly-increasing stamps on a given peer.
export class Hlc {
  constructor(peer) {
    this.peer = peer;
    this.lastWall = 0;
    this.lastLogical = 0;
  }
  send(nowMicros) {
    if (nowMicros > this.lastWall) {
      this.lastWall = nowMicros;
      this.lastLogical = 0;
    } else {
      this.lastLogical += 1;
    }
    return new HlcStamp(this.lastWall, this.lastLogical, this.peer);
  }
  // Observe a remote stamp, advancing the local clock past it (return value
  // unused by SeqCrdt.merge — only the side effect matters).
  recv(remote, nowMicros) {
    const wall = Math.max(this.lastWall, remote.wallTime, nowMicros);
    if (wall === this.lastWall && wall === remote.wallTime) {
      this.lastLogical = Math.max(this.lastLogical, remote.logical) + 1;
    } else if (wall === this.lastWall) {
      this.lastLogical += 1;
    } else if (wall === remote.wallTime) {
      this.lastLogical = remote.logical + 1;
    } else {
      this.lastLogical = 0;
    }
    this.lastWall = wall;
    return new HlcStamp(this.lastWall, this.lastLogical, this.peer);
  }
}

// LWW register: overwrite iff stamp is STRICTLY > current (incumbent wins ties).
export class LwwRegister {
  constructor(value, stamp) {
    this.value = value;
    this.stamp = stamp;
  }
  set(value, stamp) {
    if (stamp.compareTo(this.stamp) > 0) {
      this.value = value;
      this.stamp = stamp;
      return true;
    }
    return false;
  }
  // Merge from another register: take other's (value, stamp) iff strictly newer.
  // Returns true iff the VALUE changed.
  mergeFrom(other) {
    if (other.stamp.compareTo(this.stamp) > 0) {
      const changed = !valuesEqual(this.value, other.value);
      this.value = other.value;
      this.stamp = other.stamp;
      return changed;
    }
    return false;
  }
}

// Structural deep equality (#lzseqstringifyeq): replaces JSON.stringify-based
// comparison (one of V8's slowest paths) with a typed dispatch that handles the
// common cases — primitives, Position, arrays, plain objects — without
// allocating a serialized string per side per comparison. Behavior is
// equivalent to JSON.stringify equality for plain JSON values, while skipping
// JSON's NaN/Infinity/null coercion and toJSON dispatch (neither is exercised
// by the values stored in SeqCrdt registers today).
function valuesEqual(a, b) {
  if (a === b) {
    return true;
  }
  if (a instanceof Position && b instanceof Position) {
    return a.compareTo(b) === 0;
  }
  if (a === null || b === null || typeof a !== "object" || typeof b !== "object") {
    return a === b;
  }
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) {
      return false;
    }
    for (let i = 0; i < a.length; i++) {
      if (!valuesEqual(a[i], b[i])) {
        return false;
      }
    }
    return true;
  }
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  if (ka.length !== kb.length) {
    return false;
  }
  for (let i = 0; i < ka.length; i++) {
    const k = ka[i];
    if (!Object.prototype.hasOwnProperty.call(b, k)) {
      return false;
    }
    if (!valuesEqual(a[k], b[k])) {
      return false;
    }
  }
  return true;
}

// Position: fractional-index byte key + originating peer.
export class Position {
  constructor(frac, peer) {
    this.frac = frac; // number[] (bytes 0..255)
    this.peer = peer;
    Object.freeze(this);
  }
  compareTo(other) {
    const len = Math.min(this.frac.length, other.frac.length);
    for (let i = 0; i < len; i++) {
      if (this.frac[i] !== other.frac[i]) {
        return this.frac[i] < other.frac[i] ? -1 : 1;
      }
    }
    if (this.frac.length !== other.frac.length) {
      return this.frac.length < other.frac.length ? -1 : 1;
    }
    if (this.peer !== other.peer) {
      return this.peer < other.peer ? -1 : 1;
    }
    return 0;
  }
}

// Fractional-index midpoint: a key strictly between lo and hi (exclusive).
// Append-only — positions grow under repeated subdivision, never rebalanced.
function keyBetween(lo, hi) {
  const result = [];
  const cap = (lo ? lo.length : 0) + (hi ? hi.length : 0) + 2;
  let i = 0;
  while (i <= cap) {
    const a = lo && i < lo.length ? lo[i] : 0;
    const b = hi === null ? 256 : i < hi.length ? hi[i] : 0;
    if (a + 1 < b) {
      result.push(Math.floor((a + b) / 2));
      return result;
    }
    result.push(a);
    i++;
    if (a < b) {
      // Dropped strictly below hi: descend with open top.
      const loTail = lo ? lo.slice(i) : [];
      result.push(...keyBetween(loTail.length > 0 ? loTail : null, null));
      return result;
    }
    // a === b: shared prefix, continue.
  }
  result.push(128);
  return result;
}

class Entry {
  constructor(value, position, deleted) {
    this.value = value; // LwwRegister<V>
    this.position = position; // LwwRegister<Position>
    this.deleted = deleted; // LwwRegister<bool>
  }
  clone() {
    return new Entry(
      new LwwRegister(this.value.value, this.value.stamp),
      new LwwRegister(this.position.value, this.position.stamp),
      new LwwRegister(this.deleted.value, this.deleted.stamp),
    );
  }
  maxStamp() {
    let max = this.value.stamp;
    if (this.position.stamp.compareTo(max) > 0) max = this.position.stamp;
    if (this.deleted.stamp.compareTo(max) > 0) max = this.deleted.stamp;
    return max;
  }
}

export class SeqCrdt {
  #entries = new Map();
  #hlc;
  #peer;

  constructor(peer) {
    this.#peer = peer;
    this.#hlc = new Hlc(peer);
  }

  #fracOf(id) {
    const e = this.#entries.get(id);
    return e ? [...e.position.value.frac] : undefined;
  }

  insertBetween(id, value, left, right, nowMicros) {
    if (this.#entries.has(id)) {
      return; // no-op if already present (use moveBetween to relocate)
    }
    const lo = left !== null && left !== undefined ? this.#fracOf(left) : undefined;
    const hi = right !== null && right !== undefined ? this.#fracOf(right) : undefined;
    const frac = keyBetween(lo ?? null, hi ?? null);
    const pos = new Position(frac, this.#peer);
    const stamp = this.#hlc.send(nowMicros);
    this.#entries.set(id, new Entry(
      new LwwRegister(value, stamp),
      new LwwRegister(pos, stamp),
      new LwwRegister(false, stamp),
    ));
  }

  insertBack(id, value, nowMicros) {
    const order = this.order();
    const left = order.length > 0 ? order[order.length - 1] : null;
    this.insertBetween(id, value, left, null, nowMicros);
  }

  insertFront(id, value, nowMicros) {
    const order = this.order();
    const right = order.length > 0 ? order[0] : null;
    this.insertBetween(id, value, null, right, nowMicros);
  }

  setValue(id, value, nowMicros) {
    const e = this.#entries.get(id);
    if (!e) {
      return false;
    }
    return e.value.set(value, this.#hlc.send(nowMicros));
  }

  moveBetween(id, left, right, nowMicros) {
    const e = this.#entries.get(id);
    if (!e) {
      return false;
    }
    const lo = left !== null && left !== undefined ? this.#fracOf(left) : undefined;
    const hi = right !== null && right !== undefined ? this.#fracOf(right) : undefined;
    const frac = keyBetween(lo ?? null, hi ?? null);
    const pos = new Position(frac, this.#peer);
    e.position.set(pos, this.#hlc.send(nowMicros));
    return true;
  }

  moveAfter(id, anchor, nowMicros) {
    const order = this.order();
    const idx = order.indexOf(anchor);
    if (idx === -1) {
      return false;
    }
    const right = idx + 1 < order.length ? order[idx + 1] : null;
    return this.moveBetween(id, anchor, right, nowMicros);
  }

  moveBefore(id, anchor, nowMicros) {
    const order = this.order();
    const idx = order.indexOf(anchor);
    if (idx === -1) {
      return false;
    }
    const left = idx > 0 ? order[idx - 1] : null;
    return this.moveBetween(id, left, anchor, nowMicros);
  }

  remove(id, nowMicros) {
    const e = this.#entries.get(id);
    if (!e) {
      return false;
    }
    return e.deleted.set(true, this.#hlc.send(nowMicros));
  }

  contains(id) {
    const e = this.#entries.get(id);
    return e !== undefined && !e.deleted.value;
  }

  get(id) {
    const e = this.#entries.get(id);
    return e && !e.deleted.value ? e.value.value : undefined;
  }

  order() {
    const live = [];
    for (const [id, e] of this.#entries) {
      if (!e.deleted.value) {
        live.push([id, e.position.value]);
      }
    }
    live.sort((a, b) => a[1].compareTo(b[1]));
    return live.map(([id]) => id);
  }

  values() {
    return this.order().map((id) => [id, this.get(id)]);
  }

  tombstoneCount() {
    let count = 0;
    for (const e of this.#entries.values()) {
      if (e.deleted.value) {
        count++;
      }
    }
    return count;
  }

  // Deep-copy entries, mint a fresh Hlc for a new peer (stamps live in registers).
  fork(peer) {
    const copy = new SeqCrdt(peer);
    for (const [id, e] of this.#entries) {
      copy.#entries.set(id, e.clone());
    }
    return copy;
  }

  clone() {
    return this.fork(this.#peer);
  }

  // State-based merge: advance clock past everything observed, then per-element
  // three-way LWW merge over each independent register. Returns whether anything
  // changed. Commutative, associative, idempotent.
  merge(other, nowMicros) {
    for (const e of other.#entries.values()) {
      this.#hlc.recv(e.maxStamp(), nowMicros);
    }
    let changed = false;
    for (const [id, oe] of other.#entries) {
      const existing = this.#entries.get(id);
      if (existing) {
        changed = existing.value.mergeFrom(oe.value) || changed;
        changed = existing.position.mergeFrom(oe.position) || changed;
        changed = existing.deleted.mergeFrom(oe.deleted) || changed;
      } else {
        this.#entries.set(id, oe.clone());
        changed = true;
      }
    }
    return changed;
  }

  // Tombstone GC: collect entries where deleted == true AND the delete stamp is
  // causally stable (caller-supplied policy). Observationally inert: order() and
  // contains() already skip tombstones.
  gcWith(isStable) {
    const before = this.#entries.size;
    for (const [id, e] of this.#entries) {
      if (e.deleted.value && isStable(e.deleted.stamp)) {
        this.#entries.delete(id);
      }
    }
    return before - this.#entries.size;
  }

  gc(watermark) {
    return this.gcWith((s) => s.compareTo(watermark) <= 0);
  }

  // Test/fixture access to the internal entry map.
  entryCount() {
    return this.#entries.size;
  }
}
