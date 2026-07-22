// Membership + failure detection (#lzmemb) — the JS port.
//
// See `lazily-spec/docs/membership.md` and the formal model
// `lazily-formal/LazilyFormal/Membership.lean`. A MembershipCell is a reactive
// view of the live peer set backed by SWIM-style heartbeats + a Phi-accrual
// failure detector; the derived peerSet is the Alive peers. The pure core
// (MembershipCore + PhiAccrual) is the phi math + SWIM state machine, split from
// the reactive cell projecting the alive set onto a Context cell.

/** Per-peer liveness state (SWIM). */
export const PeerState = Object.freeze({
  Alive: "Alive",
  Suspect: "Suspect",
  Dead: "Dead",
  Left: "Left",
});

/** Default detector + state-machine tunables. */
export const defaultMembershipConfig = Object.freeze({
  phiThreshold: 8.0,
  suspectTimeout: 5,
  maxSamples: 100,
  minStd: 0.1,
});

/** Phi-accrual failure detector; `phi` uses the bit-portable Akka logistic
 *  approximation of the normal CDF so every binding agrees. */
export class PhiAccrual {
  constructor(maxSamples, minStd) {
    this.maxSamples = Math.max(1, maxSamples);
    this.minStd = minStd;
    this.window = [];
    this.lastHeartbeat = null;
  }
  heartbeat(now) {
    if (this.lastHeartbeat !== null) {
      this.window.push(now - this.lastHeartbeat);
      while (this.window.length > this.maxSamples) this.window.shift();
    }
    this.lastHeartbeat = now;
  }
  #mean() {
    return this.window.reduce((a, b) => a + b, 0) / this.window.length;
  }
  #std(mean) {
    const v = this.window.reduce((a, x) => a + (x - mean) * (x - mean), 0) / this.window.length;
    return Math.max(Math.sqrt(v), this.minStd);
  }
  phi(now) {
    if (this.lastHeartbeat === null || this.window.length === 0) return 0.0;
    const elapsed = now - this.lastHeartbeat;
    const mean = this.#mean();
    const std = this.#std(mean);
    const y = (elapsed - mean) / std;
    const e = Math.exp(-y * (1.5976 + 0.070566 * y * y));
    return elapsed > mean ? -Math.log10(e / (1 + e)) : -Math.log10(1 - 1 / (1 + e));
  }
}

/** The pure membership compute core: the SWIM state machine over a keyed peer
 *  map, driven by heartbeats and a logical clock. Emits PeerChangeEvents:
 *  `{ type: "Joined"|"Left"|"StateChanged", peer, from?, to? }`. */
export class MembershipCore {
  constructor(config = defaultMembershipConfig) {
    this.config = { ...defaultMembershipConfig, ...config };
    this.peers = new Map(); // peer -> { state, detector, suspectSince }
  }
  #newDetector() {
    return new PhiAccrual(this.config.maxSamples, this.config.minStd);
  }
  /** The alive peer set, sorted ascending (matches the spec's BTreeSet). */
  aliveSet() {
    return [...this.peers.entries()]
      .filter(([, r]) => r.state === PeerState.Alive)
      .map(([p]) => p)
      .sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  }
  state(peer) {
    return this.peers.get(peer)?.state ?? null;
  }
  join(peer, now) {
    const detector = this.#newDetector();
    detector.heartbeat(now);
    const prev = this.peers.get(peer)?.state ?? null;
    this.peers.set(peer, { state: PeerState.Alive, detector, suspectSince: null });
    if (prev === null) return [{ type: "Joined", peer }];
    if (prev === PeerState.Alive) return [];
    return [{ type: "StateChanged", peer, from: prev, to: PeerState.Alive }];
  }
  heartbeat(peer, now) {
    const record = this.peers.get(peer);
    if (record === undefined) return this.join(peer, now);
    record.detector.heartbeat(now);
    const from = record.state;
    if (from !== PeerState.Alive && from !== PeerState.Left) {
      record.state = PeerState.Alive;
      record.suspectSince = null;
      return [{ type: "StateChanged", peer, from, to: PeerState.Alive }];
    }
    return [];
  }
  leave(peer, _now) {
    const record = this.peers.get(peer);
    if (record === undefined || record.state === PeerState.Left) return [];
    record.state = PeerState.Left;
    record.suspectSince = null;
    return [{ type: "Left", peer }];
  }
  tick(now) {
    const events = [];
    for (const [peer, record] of this.peers) {
      if (record.state === PeerState.Alive) {
        if (record.detector.phi(now) > this.config.phiThreshold) {
          record.state = PeerState.Suspect;
          record.suspectSince = now;
          events.push({ type: "StateChanged", peer, from: PeerState.Alive, to: PeerState.Suspect });
        }
      } else if (record.state === PeerState.Suspect) {
        if (record.suspectSince !== null && now - record.suspectSince >= this.config.suspectTimeout) {
          record.state = PeerState.Dead;
          events.push({ type: "StateChanged", peer, from: PeerState.Suspect, to: PeerState.Dead });
        }
      }
    }
    return events;
  }
}

/** Compare two alive-set arrays for equality (used by the reactive cell). */
function setEquals(a, b) {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

/** Reactive membership: drives a MembershipCore and projects the alive set onto
 *  a Context cell so peerSet invalidates only on a set change. */
export class MembershipCell {
  constructor(ctx, config = defaultMembershipConfig) {
    this.ctx = ctx;
    this.core = new MembershipCore(config);
    this.peerSetCell = ctx.source([]);
  }
  #refresh() {
    const next = this.core.aliveSet();
    // Only write when the set changed, so the reader's PartialEq guard holds even
    // though the array identity differs each call.
    if (!setEquals(this.ctx.get(this.peerSetCell), next)) {
      this.ctx.set(this.peerSetCell, next);
    }
  }
  join(peer, now) {
    const ev = this.core.join(peer, now);
    this.#refresh();
    return ev;
  }
  heartbeat(peer, now) {
    const ev = this.core.heartbeat(peer, now);
    this.#refresh();
    return ev;
  }
  leave(peer, now) {
    const ev = this.core.leave(peer, now);
    this.#refresh();
    return ev;
  }
  tick(now) {
    const ev = this.core.tick(now);
    this.#refresh();
    return ev;
  }
  peerSet() {
    return this.ctx.get(this.peerSetCell);
  }
  state(peer) {
    return this.core.state(peer);
  }
}
