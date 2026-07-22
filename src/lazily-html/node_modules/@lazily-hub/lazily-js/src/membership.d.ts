// Membership + failure detection (#lzmemb) — the JS port.
// See membership.js and `lazily-spec/docs/membership.md`.

import type { Context, CellHandle } from "./reactive.js";

export type PeerStateLabel = "Alive" | "Suspect" | "Dead" | "Left";
export const PeerState: Readonly<{
  Alive: "Alive";
  Suspect: "Suspect";
  Dead: "Dead";
  Left: "Left";
}>;

export interface MembershipConfig {
  phiThreshold: number;
  suspectTimeout: number;
  maxSamples: number;
  minStd: number;
}
export const defaultMembershipConfig: Readonly<MembershipConfig>;

export type PeerChangeEvent<P> =
  | { type: "Joined"; peer: P }
  | { type: "Left"; peer: P }
  | { type: "StateChanged"; peer: P; from: PeerStateLabel; to: PeerStateLabel };

export class PhiAccrual {
  constructor(maxSamples: number, minStd: number);
  heartbeat(now: number): void;
  phi(now: number): number;
}

export class MembershipCore<P = unknown> {
  constructor(config?: Partial<MembershipConfig>);
  aliveSet(): P[];
  state(peer: P): PeerStateLabel | null;
  join(peer: P, now: number): PeerChangeEvent<P>[];
  heartbeat(peer: P, now: number): PeerChangeEvent<P>[];
  leave(peer: P, now: number): PeerChangeEvent<P>[];
  tick(now: number): PeerChangeEvent<P>[];
}

export class MembershipCell<P = unknown> {
  constructor(ctx: Context, config?: Partial<MembershipConfig>);
  readonly peerSetCell: CellHandle<P[]>;
  join(peer: P, now: number): PeerChangeEvent<P>[];
  heartbeat(peer: P, now: number): PeerChangeEvent<P>[];
  leave(peer: P, now: number): PeerChangeEvent<P>[];
  tick(now: number): PeerChangeEvent<P>[];
  peerSet(): P[];
  state(peer: P): PeerStateLabel | null;
}
