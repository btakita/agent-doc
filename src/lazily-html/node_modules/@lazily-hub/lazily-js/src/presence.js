// Presence + ephemeral plane (#lzpresence) — the JS port.
//
// See `lazily-spec/docs/presence.md` and the formal model
// `lazily-formal/LazilyFormal/Presence.lean`. The CRDT plane is durable;
// collaborative apps also need an ephemeral plane that does not persist (live
// cursors, presence). Each primitive is a pure compute core (a keyed map /
// single value + TTL) split from a reactive cell projecting the live view.
//
// The ephemeral plane is distinct from the durable plane; ephemeral values carry
// `plane: "ephemeral"` so a durable sink can reject them (JS has no static
// generics, so the guard is a runtime tag). A value of `null` means "no value".

/** Plane tags for the ephemeral / durable split. */
export const Plane = Object.freeze({ Ephemeral: "ephemeral", Durable: "durable" });

// ---------------------------------------------------------------------------
// Ephemeral single value
// ---------------------------------------------------------------------------

/** Single-value auto-expiry core — "the last value seen in window N". */
export class EphemeralCore {
  constructor() {
    this.plane = Plane.Ephemeral;
    this.val = null;
    this.expiry = 0;
  }
  set(value, now, ttl) {
    this.val = value;
    this.expiry = now + ttl;
  }
  tick(now) {
    if (this.val !== null && now >= this.expiry) this.val = null;
  }
  value() {
    return this.val;
  }
}

/** Reactive single-value ephemeral cell. */
export class EphemeralCell {
  constructor(ctx) {
    this.plane = Plane.Ephemeral;
    this.ctx = ctx;
    this.core = new EphemeralCore();
    this.valueCell = ctx.source(null);
  }
  #refresh() {
    this.ctx.set(this.valueCell, this.core.value());
  }
  set(value, now, ttl) {
    this.core.set(value, now, ttl);
    this.#refresh();
  }
  tick(now) {
    this.core.tick(now);
    this.#refresh();
  }
  value() {
    return this.ctx.get(this.valueCell);
  }
}

// ---------------------------------------------------------------------------
// Keyed per-peer ephemeral map (presence + awareness)
// ---------------------------------------------------------------------------

function mapEquals(a, b) {
  // #lzjsshalloweq: shallow key->value equality with an index loop (no
  // closure). Short-circuits on the first mismatched value.
  const ak = Object.keys(a);
  const bk = Object.keys(b);
  if (ak.length !== bk.length) return false;
  for (let i = 0; i < ak.length; i++) {
    const k = ak[i];
    if (a[k] !== b[k]) return false;
  }
  return true;
}

/** Per-key ephemeral map with TTL eviction — shared by presence and awareness. */
export class EphemeralMapCore {
  constructor() {
    this.plane = Plane.Ephemeral;
    this.entries = new Map(); // key -> { value, expiry }
  }
  set(key, value, now, ttl) {
    this.entries.set(key, { value, expiry: now + ttl });
  }
  evict(key) {
    this.entries.delete(key);
  }
  tick(now) {
    for (const [k, e] of this.entries) if (now >= e.expiry) this.entries.delete(k);
  }
  get(key, now) {
    const e = this.entries.get(key);
    return e !== undefined && now < e.expiry ? e.value : null;
  }
  /** The live key -> value object, keys sorted ascending (numeric or string). */
  present(now) {
    const out = {};
    const keys = [...this.entries.keys()]
      .filter((k) => now < this.entries.get(k).expiry)
      .sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
    for (const k of keys) out[k] = this.entries.get(k).value;
    return out;
  }
}

/** Shared reactive wrapper for the keyed ephemeral map. */
class EphemeralMapCell {
  constructor(ctx, ttl) {
    this.plane = Plane.Ephemeral;
    this.ctx = ctx;
    this.ttl = ttl;
    this.core = new EphemeralMapCore();
    this.presentCell = ctx.source({});
  }
  _refresh(now) {
    const next = this.core.present(now);
    if (!mapEquals(this.ctx.get(this.presentCell), next)) {
      this.ctx.set(this.presentCell, next);
    }
  }
  present() {
    return this.ctx.get(this.presentCell);
  }
  get(peer, now) {
    return this.core.get(peer, now);
  }
}

/** Reactive per-peer presence: heartbeat-kept, membership- and TTL-evicted. */
export class PresenceCell extends EphemeralMapCell {
  heartbeat(peer, value, now) {
    this.core.set(peer, value, now, this.ttl);
    this._refresh(now);
  }
  evict(peer, now) {
    this.core.evict(peer);
    this._refresh(now);
  }
  tick(now) {
    this.core.tick(now);
    this._refresh(now);
  }
}

/** Reactive typed ephemeral broadcast (cursors): last-writer-per-peer with TTL. */
export class AwarenessCell extends EphemeralMapCell {
  set(peer, value, now) {
    this.core.set(peer, value, now, this.ttl);
    this._refresh(now);
  }
  tick(now) {
    this.core.tick(now);
    this._refresh(now);
  }
}
