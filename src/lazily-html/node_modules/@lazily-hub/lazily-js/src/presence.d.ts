// Presence + ephemeral plane (#lzpresence) — the JS port.
// See presence.js and `lazily-spec/docs/presence.md`.

import type { Context, CellHandle } from "./reactive.js";

export const Plane: Readonly<{ Ephemeral: "ephemeral"; Durable: "durable" }>;

export class EphemeralCore<T = unknown> {
  readonly plane: "ephemeral";
  set(value: T, now: number, ttl: number): void;
  tick(now: number): void;
  value(): T | null;
}

export class EphemeralCell<T = unknown> {
  constructor(ctx: Context);
  readonly plane: "ephemeral";
  readonly valueCell: CellHandle<T | null>;
  set(value: T, now: number, ttl: number): void;
  tick(now: number): void;
  value(): T | null;
}

export class EphemeralMapCore<K = unknown, V = unknown> {
  readonly plane: "ephemeral";
  set(key: K, value: V, now: number, ttl: number): void;
  evict(key: K): void;
  tick(now: number): void;
  get(key: K, now: number): V | null;
  present(now: number): Record<string, V>;
}

export class PresenceCell<K = unknown, V = unknown> {
  constructor(ctx: Context, ttl: number);
  readonly plane: "ephemeral";
  readonly presentCell: CellHandle<Record<string, V>>;
  heartbeat(peer: K, value: V, now: number): void;
  evict(peer: K, now: number): void;
  tick(now: number): void;
  present(): Record<string, V>;
  get(peer: K, now: number): V | null;
}

export class AwarenessCell<K = unknown, V = unknown> {
  constructor(ctx: Context, ttl: number);
  readonly plane: "ephemeral";
  readonly presentCell: CellHandle<Record<string, V>>;
  set(peer: K, value: V, now: number): void;
  tick(now: number): void;
  present(): Record<string, V>;
  get(peer: K, now: number): V | null;
}
