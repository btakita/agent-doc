// Embedded-service plane (#lzservice) — the JS port.
// See service.js and `lazily-spec/docs/service.md`.

import type { Context, CellHandle } from "./reactive.js";

export type HealthLabel = "Healthy" | "Degraded" | "Unhealthy";
export const Health: Readonly<{ Healthy: "Healthy"; Degraded: "Degraded"; Unhealthy: "Unhealthy" }>;

export class HealthCore {
  set(name: string, up: boolean, critical: boolean): void;
  health(): HealthLabel;
}
export class HealthCell {
  constructor(ctx: Context);
  readonly healthCell: CellHandle<HealthLabel>;
  set(name: string, up: boolean, critical: boolean): void;
  health(): HealthLabel;
}

export class ReadinessCore {
  set(name: string, ready: boolean): void;
  ready(): boolean;
}
export class ReadinessCell {
  constructor(ctx: Context);
  readonly readyCell: CellHandle<boolean>;
  set(name: string, ready: boolean): void;
  ready(): boolean;
}

export class DiscoveryCore<P = unknown> {
  register(service: string, endpoint: string, peer: P): void;
  deregister(service: string): void;
  evict(peer: P): void;
  resolve(service: string): string | null;
  discovery(): Record<string, string>;
}
export class DiscoveryCell<P = unknown> {
  constructor(ctx: Context);
  readonly discoveryCell: CellHandle<Record<string, string>>;
  register(service: string, endpoint: string, peer: P): void;
  deregister(service: string): void;
  evict(peer: P): void;
  resolve(service: string): string | null;
  discovery(): Record<string, string>;
}

export class ServiceRegistryCore {
  register(service: string, endpoint: string): void;
  deregister(service: string): void;
  replay(): void;
  projectionObject(): Record<string, string>;
}
export class ServiceRegistry {
  constructor(ctx: Context);
  readonly projectionCell: CellHandle<Record<string, string>>;
  register(service: string, endpoint: string): void;
  deregister(service: string): void;
  replay(): void;
  projection(): Record<string, string>;
}
