// Distributed coordination (#lzcoord) — the JS port.
//
// See `lazily-spec/docs/coordination.md` and the formal model
// `lazily-formal/LazilyFormal/Coordination.lean`. Lease / leader / lock /
// semaphore / barrier + quorum primitives, each a pure compute core split from a
// reactive cell projecting the salient reader. Time is the logical clock. A
// holder of `null` means "no holder".

// ---------------------------------------------------------------------------
// Lease + fencing token
// ---------------------------------------------------------------------------

/** Single-writer lease authority with a monotone fencing token. */
export class LeaseCore {
  constructor() {
    this.holderPeer = null;
    this.expiry = 0;
    this.fence = 0;
  }
  #isExpired(now) {
    return this.holderPeer !== null && now >= this.expiry;
  }
  isHeld(now) {
    return this.holderPeer !== null && !this.#isExpired(now);
  }
  holder(now) {
    return this.isHeld(now) ? this.holderPeer : null;
  }
  acquire(peer, now, ttl) {
    if (this.holderPeer === null || this.#isExpired(now)) {
      this.fence += 1;
      this.holderPeer = peer;
      this.expiry = now + ttl;
      return this.fence;
    }
    if (this.holderPeer === peer) {
      this.expiry = now + ttl; // renew keeps fence
      return this.fence;
    }
    return null;
  }
  renew(peer, now, ttl) {
    if (this.isHeld(now) && this.holderPeer === peer) {
      this.expiry = now + ttl;
      return true;
    }
    return false;
  }
  release(peer) {
    if (this.holderPeer === peer) this.holderPeer = null;
  }
  tick(now) {
    if (this.#isExpired(now)) {
      this.holderPeer = null;
      return true;
    }
    return false;
  }
}

/** Reactive lease: projects the holder onto a cell. */
export class LeaseCell {
  constructor(ctx) {
    this.ctx = ctx;
    this.core = new LeaseCore();
    this.holderCell = ctx.source(null);
  }
  #refresh(now) {
    this.ctx.set(this.holderCell, this.core.holder(now));
  }
  acquire(peer, now, ttl) {
    const r = this.core.acquire(peer, now, ttl);
    this.#refresh(now);
    return r;
  }
  renew(peer, now, ttl) {
    const r = this.core.renew(peer, now, ttl);
    this.#refresh(now);
    return r;
  }
  release(peer, now) {
    this.core.release(peer);
    this.#refresh(now);
  }
  tick(now) {
    const r = this.core.tick(now);
    this.#refresh(now);
    return r;
  }
  holder(now) {
    return this.core.holder(now);
  }
  isHeld(now) {
    return this.core.isHeld(now);
  }
  fence() {
    return this.core.fence;
  }
}

// ---------------------------------------------------------------------------
// Leader / follower / candidate
// ---------------------------------------------------------------------------

export const LeaderRole = Object.freeze({
  Leader: "Leader",
  Follower: "Follower",
  Candidate: "Candidate",
});

/** Reactive leadership over a lease from node `me`'s perspective. */
export class LeaderCell {
  constructor(ctx, me) {
    this.ctx = ctx;
    this.me = me;
    this.core = new LeaseCore();
    this.currentLeaderCell = ctx.source(null);
  }
  #refresh(now) {
    this.ctx.set(this.currentLeaderCell, this.core.holder(now));
  }
  campaign(now, ttl) {
    this.core.acquire(this.me, now, ttl);
    this.#refresh(now);
    return this.role(now);
  }
  contend(peer, now, ttl) {
    this.core.acquire(peer, now, ttl);
    this.#refresh(now);
    return this.role(now);
  }
  tick(now) {
    this.core.tick(now);
    this.#refresh(now);
    return this.role(now);
  }
  currentLeader(now) {
    return this.core.holder(now);
  }
  role(now) {
    const h = this.core.holder(now);
    if (h === null) return LeaderRole.Candidate;
    return h === this.me ? LeaderRole.Leader : LeaderRole.Follower;
  }
}

// ---------------------------------------------------------------------------
// Distributed lock + fencing
// ---------------------------------------------------------------------------

/** Reactive distributed mutex over a lease + fencing token. */
export class LockCell {
  constructor(ctx) {
    this.ctx = ctx;
    this.core = new LeaseCore();
    this.isLockedCell = ctx.source(false);
  }
  #refresh(now) {
    this.ctx.set(this.isLockedCell, this.core.isHeld(now));
  }
  acquire(peer, now, ttl) {
    const r = this.core.acquire(peer, now, ttl);
    this.#refresh(now);
    return r;
  }
  release(peer, now) {
    this.core.release(peer);
    this.#refresh(now);
  }
  tick(now) {
    const r = this.core.tick(now);
    this.#refresh(now);
    return r;
  }
  /** Whether `fence` is the current (non-stale) fencing token. */
  validate(fence) {
    return this.core.fence === fence;
  }
  isLocked(now) {
    return this.core.isHeld(now);
  }
  fence() {
    return this.core.fence;
  }
}

// ---------------------------------------------------------------------------
// Semaphore
// ---------------------------------------------------------------------------

/** Bounded permit pool compute core. */
export class SemaphoreCore {
  constructor(capacity) {
    this.capacity = capacity;
    this.acquired = 0;
  }
  available() {
    return this.capacity - this.acquired;
  }
  acquire() {
    if (this.acquired < this.capacity) {
      this.acquired += 1;
      return true;
    }
    return false;
  }
  release() {
    if (this.acquired > 0) this.acquired -= 1;
  }
}

/** Reactive semaphore: projects permitsAvailable onto a cell. */
export class SemaphoreCell {
  constructor(ctx, capacity) {
    this.ctx = ctx;
    this.core = new SemaphoreCore(capacity);
    this.permitsAvailableCell = ctx.source(capacity);
  }
  #refresh() {
    this.ctx.set(this.permitsAvailableCell, this.core.available());
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
  permitsAvailable() {
    return this.ctx.get(this.permitsAvailableCell);
  }
}

// ---------------------------------------------------------------------------
// Barrier / quorum
// ---------------------------------------------------------------------------

/** Wait-for-N gate over distinct arriving peers. */
export class BarrierCore {
  constructor(required) {
    this.required = required;
    this.arrived = new Set();
  }
  arrive(peer) {
    this.arrived.add(peer);
    return this.isOpen();
  }
  count() {
    return this.arrived.size;
  }
  isOpen() {
    return this.count() >= this.required;
  }
}

/** Reactive wait-for-N gate. A quorum is a barrier with required = total/2 + 1. */
export class BarrierCell {
  constructor(ctx, required) {
    this.ctx = ctx;
    this.core = new BarrierCore(required);
    this.isOpenCell = ctx.source(this.core.isOpen());
  }
  /** A quorum gate: opens at strict majority of `total`. */
  static quorum(ctx, total) {
    return new BarrierCell(ctx, Math.floor(total / 2) + 1);
  }
  #refresh() {
    this.ctx.set(this.isOpenCell, this.core.isOpen());
  }
  arrive(peer) {
    const r = this.core.arrive(peer);
    this.#refresh();
    return r;
  }
  count() {
    return this.core.count();
  }
  isOpen() {
    return this.ctx.get(this.isOpenCell);
  }
}
