// Embedded-service plane (#lzservice) — the JS port.
//
// See `lazily-spec/docs/service.md` and the formal model
// `lazily-formal/LazilyFormal/Service.lean`. HealthCell / ReadinessCell /
// DiscoveryCell / ServiceRegistry, each a pure compute core split from a
// reactive cell projecting the composed view.

function objectEquals(a, b) {
  const ak = Object.keys(a);
  const bk = Object.keys(b);
  if (ak.length !== bk.length) return false;
  for (const k of ak) if (a[k] !== b[k]) return false;
  return true;
}

function sortedObject(entries) {
  const out = {};
  for (const k of [...entries.keys()].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0))) {
    out[k] = entries.get(k);
  }
  return out;
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

export const Health = Object.freeze({
  Healthy: "Healthy",
  Degraded: "Degraded",
  Unhealthy: "Unhealthy",
});

/** Composed liveness-probe core: worst component dominates. */
export class HealthCore {
  constructor() {
    this.probes = new Map(); // name -> { up, critical }
  }
  set(name, up, critical) {
    this.probes.set(name, { up, critical });
  }
  health() {
    let anyDown = false;
    for (const { up, critical } of this.probes.values()) {
      if (!up && critical) return Health.Unhealthy;
      if (!up) anyDown = true;
    }
    return anyDown ? Health.Degraded : Health.Healthy;
  }
}

/** Reactive health: projects the aggregate onto a cell for /health. */
export class HealthCell {
  constructor(ctx) {
    this.ctx = ctx;
    this.core = new HealthCore();
    this.healthCell = ctx.source(Health.Healthy);
  }
  #refresh() {
    this.ctx.set(this.healthCell, this.core.health());
  }
  set(name, up, critical) {
    this.core.set(name, up, critical);
    this.#refresh();
  }
  health() {
    return this.core.health();
  }
}

// ---------------------------------------------------------------------------
// Readiness
// ---------------------------------------------------------------------------

/** Composed readiness-probe core: ready iff every condition holds. */
export class ReadinessCore {
  constructor() {
    this.conditions = new Map();
  }
  set(name, ready) {
    this.conditions.set(name, ready);
  }
  ready() {
    for (const r of this.conditions.values()) if (!r) return false;
    return true;
  }
}

/** Reactive readiness: projects ready onto a cell for /ready. */
export class ReadinessCell {
  constructor(ctx) {
    this.ctx = ctx;
    this.core = new ReadinessCore();
    this.readyCell = ctx.source(true);
  }
  #refresh() {
    this.ctx.set(this.readyCell, this.core.ready());
  }
  set(name, ready) {
    this.core.set(name, ready);
    this.#refresh();
  }
  ready() {
    return this.core.ready();
  }
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/** Service-discovery core: service -> { endpoint, peer }. A peer's departure
 *  (evict) removes its endpoints. */
export class DiscoveryCore {
  constructor() {
    this.entries = new Map();
  }
  register(service, endpoint, peer) {
    this.entries.set(service, { endpoint, peer });
  }
  deregister(service) {
    this.entries.delete(service);
  }
  evict(peer) {
    for (const [s, e] of this.entries) if (e.peer === peer) this.entries.delete(s);
  }
  resolve(service) {
    const e = this.entries.get(service);
    return e === undefined ? null : e.endpoint;
  }
  discovery() {
    const m = new Map();
    for (const [s, e] of this.entries) m.set(s, e.endpoint);
    return sortedObject(m);
  }
}

/** Reactive service discovery. */
export class DiscoveryCell {
  constructor(ctx) {
    this.ctx = ctx;
    this.core = new DiscoveryCore();
    this.discoveryCell = ctx.source({});
  }
  #refresh() {
    const next = this.core.discovery();
    if (!objectEquals(this.ctx.get(this.discoveryCell), next)) {
      this.ctx.set(this.discoveryCell, next);
    }
  }
  register(service, endpoint, peer) {
    this.core.register(service, endpoint, peer);
    this.#refresh();
  }
  deregister(service) {
    this.core.deregister(service);
    this.#refresh();
  }
  evict(peer) {
    this.core.evict(peer);
    this.#refresh();
  }
  resolve(service) {
    return this.core.resolve(service);
  }
  discovery() {
    return this.ctx.get(this.discoveryCell);
  }
}

// ---------------------------------------------------------------------------
// Service registry (durable)
// ---------------------------------------------------------------------------

/** Durable service-registry core: an ordered log whose left-fold is the
 *  projection, so replay reconstructs it. */
export class ServiceRegistryCore {
  constructor() {
    this.log = []; // { type: "register"|"deregister", service, endpoint? }
    this.projection = new Map();
  }
  #apply(projection, op) {
    if (op.type === "register") projection.set(op.service, op.endpoint);
    else projection.delete(op.service);
  }
  register(service, endpoint) {
    const op = { type: "register", service, endpoint };
    this.#apply(this.projection, op);
    this.log.push(op);
  }
  deregister(service) {
    const op = { type: "deregister", service };
    this.#apply(this.projection, op);
    this.log.push(op);
  }
  replay() {
    const projection = new Map();
    for (const op of this.log) this.#apply(projection, op);
    this.projection = projection;
  }
  projectionObject() {
    return sortedObject(this.projection);
  }
}

/** Reactive durable service registry. */
export class ServiceRegistry {
  constructor(ctx) {
    this.ctx = ctx;
    this.core = new ServiceRegistryCore();
    this.projectionCell = ctx.source({});
  }
  #refresh() {
    const next = this.core.projectionObject();
    if (!objectEquals(this.ctx.get(this.projectionCell), next)) {
      this.ctx.set(this.projectionCell, next);
    }
  }
  register(service, endpoint) {
    this.core.register(service, endpoint);
    this.#refresh();
  }
  deregister(service) {
    this.core.deregister(service);
    this.#refresh();
  }
  replay() {
    this.core.replay();
    this.#refresh();
  }
  projection() {
    return this.ctx.get(this.projectionCell);
  }
}
