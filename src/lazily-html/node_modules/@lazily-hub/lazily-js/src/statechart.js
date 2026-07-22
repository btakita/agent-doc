// Full Harel/SCXML state charts — native JavaScript, conforming to
// `lazily-spec/docs/state-charts.md` and the Lean `LazilyFormal.StateChart`
// formal model (`lazily-formal`).
//
// A chart is **compute, not protocol**: it is never serialized as a distinct
// wire kind. lazily-js is a state-projection *consumer* with no reactive
// graph, so the active configuration is a plain `Set` (not a `Cell`); the
// transition is pure logic with zero system dependencies, exactly as the spec
// requires for lazily-js / lazily-kt.
//
// Implemented subset (per the spec's implementation-status note): compound
// states, orthogonal (parallel) regions, shallow + deep history, entry/exit/
// transition actions, named guards, external + internal transitions. Extended
// state `{"expr": …}` guards and `run` actions are rejected explicitly; `final`
// states are accepted as leaves without raising completion (`done`) events.

const HISTORY_SHALLOW = "shallow";
const HISTORY_DEEP = "deep";

function assertObject(value, name) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`${name} must be an object`);
  }
  return value;
}

function parseActionList(raw, label) {
  if (raw === undefined || raw === null) {
    return [];
  }
  if (!Array.isArray(raw)) {
    throw new TypeError(`${label} must be an array of strings`);
  }
  return raw.map((item) => {
    if (typeof item !== "string") {
      throw new TypeError(`${label} actions must be strings`);
    }
    return item;
  });
}

function parseTransition(raw, stateId, event) {
  if (typeof raw === "string") {
    return { target: raw, guard: null, action: [], internal: false };
  }
  const obj = assertObject(raw, `transition ${stateId}.${event}`);
  const target = obj.target;
  if (typeof target !== "string") {
    throw new TypeError(`transition ${stateId}.${event} requires a string \`target\``);
  }
  let guard = null;
  if (obj.guard !== undefined && obj.guard !== null) {
    if (typeof obj.guard === "string") {
      guard = obj.guard;
    } else if (typeof obj.guard === "object" && obj.guard !== null && "expr" in obj.guard) {
      throw new TypeError(
        `transition ${stateId}.${event}: context-expression \`{expr: …}\` guards are not supported (rejecting explicitly per spec)`,
      );
    } else {
      throw new TypeError(`transition ${stateId}.${event}: guard must be a string`);
    }
  }
  const action = parseActionList(obj.action, `transition ${stateId}.${event} action`);
  const internal = obj.internal === true;
  return { target, guard, action, internal };
}

function parseState(id, raw) {
  const obj = assertObject(raw, `state ${id}`);
  const parent = typeof obj.parent === "string" ? obj.parent : null;
  const initial = typeof obj.initial === "string" ? obj.initial : null;
  const defaultTarget = typeof obj.default === "string" ? obj.default : null;

  if (obj.run !== undefined && obj.run !== null) {
    throw new TypeError(
      `state ${id} uses \`run\` actions, which are not supported (rejecting explicitly per spec)`,
    );
  }

  let kind;
  let history = null;
  if (typeof obj.history === "string") {
    if (obj.history !== HISTORY_SHALLOW && obj.history !== HISTORY_DEEP) {
      throw new TypeError(`state ${id}: unknown history kind \`${obj.history}\``);
    }
    history = obj.history;
    kind = "history";
  } else if (obj.parallel === true) {
    kind = "parallel";
  } else if (obj.kind === "final") {
    kind = "final";
  } else if (initial !== null) {
    kind = "compound";
  } else {
    kind = "atomic";
  }

  const entry = parseActionList(obj.entry, `state ${id} entry`);
  const exit = parseActionList(obj.exit, `state ${id} exit`);

  const transitions = new Map();
  if (obj.on !== undefined && obj.on !== null) {
    const on = assertObject(obj.on, `state ${id}.on`);
    for (const [event, rawT] of Object.entries(on)) {
      transitions.set(event, parseTransition(rawT, id, event));
    }
  }

  return { id, parent, kind, history, initial, default: defaultTarget, transitions, entry, exit };
}

function computeDepth(states, id, current, depth) {
  depth.set(id, current);
  for (const def of states.values()) {
    if (def.parent === id) {
      computeDepth(states, def.id, current + 1, depth);
    }
  }
}

/**
 * A parsed, immutable chart definition.
 */
export class ChartDef {
  constructor({ states, children, order, depth, root }) {
    this.states = states;
    this.children = children;
    this.order = order;
    this.depth = depth;
    this.root = root;
    Object.freeze(this);
  }

  /**
   * Parse a chart definition from the declarative JSON form
   * (per `schemas/statechart.json`). Throws on malformed charts or unsupported
   * features (`run` actions, `{expr: …}` guards).
   *
   * @param {any} value the declarative chart object
   * @returns {ChartDef}
   */
  static fromChart(value) {
    const obj = assertObject(value, "chart");
    // `chart.initial` presence is validated; descent uses each compound's own
    // `initial` from the root, so the value itself is not stored.
    if (typeof obj.initial !== "string") {
      throw new TypeError("chart.initial is required");
    }
    const statesObj = assertObject(obj.states, "chart.states");

    const states = new Map();
    const order = new Map();
    let idx = 0;
    for (const [id, raw] of Object.entries(statesObj)) {
      order.set(id, idx);
      idx += 1;
      states.set(id, parseState(id, raw));
    }

    return ChartDef.fromStates(states, order);
  }

  /**
   * Assemble a validated {@link ChartDef} from parsed states plus their document
   * order. Shared by {@link ChartDef.fromChart} and the {@link ChartBuilder}, so
   * both definition paths derive parent→children, the single parent-less root,
   * and per-node depth identically. `order` maps each state id to the position
   * that fixes deterministic parallel-region descent.
   *
   * @param {Map<string, any>} states id → parsed state def (each carrying `id`)
   * @param {Map<string, number>} order id → document position
   * @returns {ChartDef}
   */
  static fromStates(states, order) {
    // Derived structure: children, root.
    const children = new Map();
    let root = null;
    for (const def of states.values()) {
      if (def.parent !== null) {
        let list = children.get(def.parent);
        if (!list) {
          list = [];
          children.set(def.parent, list);
        }
        list.push(def.id);
      } else {
        if (root !== null) {
          throw new TypeError("chart has more than one root (parent-less state)");
        }
        root = def.id;
      }
    }
    // Sort children by document order for deterministic parallel descent.
    for (const list of children.values()) {
      list.sort((a, b) => (order.get(a) ?? Infinity) - (order.get(b) ?? Infinity));
    }
    if (root === null) {
      throw new TypeError("chart has no root (parent-less state)");
    }

    const depth = new Map();
    computeDepth(states, root, 0, depth);

    return new ChartDef({ states, children, order, depth, root });
  }

  kind(id) {
    return this.states.get(id)?.kind ?? "atomic";
  }

  isLeaf(id) {
    const k = this.kind(id);
    return k === "atomic" || k === "final";
  }

  ancestorsInclusive(id) {
    const out = [];
    let cur = id;
    while (cur !== null) {
      const def = this.states.get(cur);
      if (!def) break;
      out.push(cur);
      cur = def.parent;
    }
    return out;
  }

  lca(a, b) {
    const ancA = new Set(this.ancestorsInclusive(a));
    for (const cid of this.ancestorsInclusive(b)) {
      if (ancA.has(cid)) {
        return cid;
      }
    }
    return this.root;
  }

  isProperDescendant(desc, anc) {
    return desc !== anc && this.ancestorsInclusive(desc).includes(anc);
  }

  depthOf(id) {
    return this.depth.get(id) ?? 0;
  }
}

function enterSubtree(def, state, enter, actions) {
  const node = def.states.get(state);
  enter.add(state);
  if (actions && node) {
    for (const a of node.entry) actions.push(a);
  }
  if (!node) return;
  switch (node.kind) {
    case "compound":
      if (node.initial !== null) {
        enterSubtree(def, node.initial, enter, actions);
      }
      break;
    case "parallel":
      for (const region of def.children.get(state) ?? []) {
        enterSubtree(def, region, enter, actions);
      }
      break;
    default:
      break;
  }
}

/** Path from just-below `lca` down to `target` (exclusive lca, inclusive target). */
function pathBelow(def, lca, target) {
  const chain = def.ancestorsInclusive(target); // [target, ..., root]
  const idx = chain.findIndex((x) => x === lca);
  const end = idx === -1 ? chain.length : idx;
  return chain.slice(0, end).reverse(); // [child-of-lca, ..., target]
}

function historyChildOf(def, region) {
  for (const k of def.children.get(region) ?? []) {
    if (def.kind(k) === "history") return k;
  }
  return null;
}

function recordRegion(def, region, histChild, config, history) {
  const histDef = def.states.get(histChild);
  if (!histDef || histDef.kind !== "history") return;
  if (histDef.history === HISTORY_SHALLOW) {
    // Record the direct child of `region` that was active.
    for (const c of def.children.get(region) ?? []) {
      if (config.has(c) && def.kind(c) !== "history") {
        history.set(histChild, { kind: "shallow", child: c });
        return;
      }
    }
  } else {
    // Record every active state strictly below `region`.
    const set = new Set();
    for (const s of config) {
      if (def.isProperDescendant(s, region)) set.add(s);
    }
    history.set(histChild, { kind: "deep", set });
  }
}

function guardPasses(transition, guards) {
  if (transition.guard === null) return true;
  return guards.get(transition.guard) === true; // fail-closed
}

/**
 * A native full-Harel state chart. The active configuration lives in a plain
 * `Set` (lazily-js is a state-projection consumer with no reactive graph).
 *
 * Deterministic by construction (mirroring the Lean `StateChart.send` total
 * function): a given `(chart, history, configuration, event, guards)` yields a
 * unique result — the confluence guarantee every binding inherits by replaying
 * the shared conformance fixtures.
 */
export class StateChart {
  #def;
  #config;
  #history;
  #lastActions;

  /**
   * Enter the initial configuration by descending from the root via each
   * compound's `initial` (and every region for parallel states). Initial entry
   * actions are recorded and available via {@link lastActions}.
   * @param {ChartDef} def
   */
  constructor(def) {
    this.#def = def;
    const enter = new Set();
    const actions = [];
    enterSubtree(def, def.root, enter, actions);
    this.#config = enter;
    this.#history = new Map();
    this.#lastActions = actions;
  }

  /** Ordered action names fired by the initial entry or the most recent
   *  {@link send} (exit → transition → entry). */
  lastActions() {
    return [...this.#lastActions];
  }

  /** The full active configuration (active leaves plus all active ancestors),
   *  as a sorted array of state ids. */
  configuration() {
    return [...this.#config].sort();
  }

  /** Active atomic leaves, sorted (one per parallel region; one for single-region). */
  activeLeaves() {
    return [...this.#config]
      .filter((id) => this.#def.isLeaf(id))
      .sort();
  }

  /** Hierarchical "state-in" predicate: `true` iff `id` is active. */
  matches(id) {
    return this.#config.has(id);
  }

  /**
   * Send an event. Returns `true` if any transition was taken, `false` if
   * rejected (configuration unchanged, no actions fired). `guards` resolves
   * named guards for this send (absent/unknown name → fail-closed `false`).
   *
   * @param {string} event
   * @param {Record<string, boolean> | Map<string, boolean>} [guards]
   * @returns {boolean}
   */
  send(event, guards = {}) {
    const guardMap = guards instanceof Map ? guards : new Map(Object.entries(guards));
    const config = this.#config;
    const def = this.#def;

    // 1. Enabled transitions: per active leaf, innermost passing match.
    const leaves = [...config].filter((id) => def.isLeaf(id));
    /** @type {{source: string, transition: object, leaf: string}[]} */
    const candidates = [];
    for (const leaf of leaves) {
      for (const anc of def.ancestorsInclusive(leaf)) {
        const stateDef = def.states.get(anc);
        const t = stateDef?.transitions.get(event);
        if (t && guardPasses(t, guardMap)) {
          candidates.push({ source: anc, transition: t, leaf });
          break; // innermost wins for this leaf's chain
        }
      }
    }

    if (candidates.length === 0) {
      this.#lastActions = [];
      return false;
    }

    // 2. Conflict resolution: order by source depth desc, then document order;
    //    take greedily, skipping any whose exit set intersects the taken union.
    candidates.sort((a, b) => {
      const byDepth = def.depthOf(b.source) - def.depthOf(a.source);
      if (byDepth !== 0) return byDepth;
      return (def.order.get(a.source) ?? Infinity) - (def.order.get(b.source) ?? Infinity);
    });

    const exitUnion = new Set();
    const enterUnion = new Set();
    const takenTransitions = [];
    for (const cand of candidates) {
      const { exitSet, enterSet } = computeExitEnter(def, cand.source, cand.transition, cand.leaf, config, this.#history);
      let conflicts = false;
      for (const s of exitSet) {
        if (exitUnion.has(s)) {
          conflicts = true;
          break;
        }
      }
      if (conflicts) continue;
      for (const s of exitSet) exitUnion.add(s);
      for (const s of enterSet) enterUnion.add(s);
      takenTransitions.push(cand.transition);
    }

    if (takenTransitions.length === 0) {
      this.#lastActions = [];
      return false;
    }

    // 3. Record history for regions being exited that own a history child.
    for (const s of exitUnion) {
      const hChild = historyChildOf(def, s);
      if (hChild) {
        recordRegion(def, s, hChild, config, this.#history);
      }
    }

    // 4. Action trace: exit (innermost-first) → transition → entry (outermost-first).
    const actions = [];
    const exitSorted = [...exitUnion].sort((a, b) => {
      const byDepth = def.depthOf(b) - def.depthOf(a); // depth desc
      if (byDepth !== 0) return byDepth;
      return a < b ? -1 : a > b ? 1 : 0; // alphabetical for stable tie-break
    });
    for (const s of exitSorted) {
      for (const a of def.states.get(s)?.exit ?? []) actions.push(a);
    }
    for (const t of takenTransitions) {
      for (const a of t.action) actions.push(a);
    }
    const enterSorted = [...enterUnion].sort((a, b) => {
      const byDepth = def.depthOf(a) - def.depthOf(b); // depth asc
      if (byDepth !== 0) return byDepth;
      return a < b ? -1 : a > b ? 1 : 0;
    });
    for (const s of enterSorted) {
      for (const a of def.states.get(s)?.entry ?? []) actions.push(a);
    }

    // 5. Apply new configuration.
    for (const s of exitUnion) config.delete(s);
    for (const s of enterUnion) config.add(s);

    this.#lastActions = actions;
    return true;
  }
}

function computeExitEnter(def, source, transition, leaf, config, history) {
  const target = transition.target;
  const internal =
    transition.internal && (target === source || def.isProperDescendant(target, source));
  const lca = internal ? source : def.lca(leaf, target);

  // Exit set: active proper-descendants of the lca.
  const exitSet = new Set();
  for (const s of config) {
    if (def.isProperDescendant(s, lca)) exitSet.add(s);
  }

  // Enter set.
  const enter = new Set();
  if (def.kind(target) === "history") {
    const targetDef = def.states.get(target);
    const region = targetDef?.parent ?? def.root;
    for (const s of pathBelow(def, lca, region)) enter.add(s);
    restoreViaHistory(def, history, target, region, enter);
  } else {
    for (const s of pathBelow(def, lca, target)) enter.add(s);
    enterSubtree(def, target, enter, null);
  }

  return { exitSet, enterSet: enter };
}

function restoreViaHistory(def, history, hist, region, enter) {
  const recording = history.get(hist);
  if (!recording) {
    // First entry: descend via `default`, else the region's `initial`.
    const histDef = def.states.get(hist);
    const regionDef = def.states.get(region);
    const start = histDef?.default ?? regionDef?.initial ?? null;
    if (start !== null) {
      for (const s of pathBelow(def, region, start)) enter.add(s);
      enterSubtree(def, start, enter, null);
    }
    return;
  }
  if (recording.kind === "shallow") {
    const child = recording.child;
    enter.add(child);
    enterSubtree(def, child, enter, null);
  } else {
    for (const s of recording.set) enter.add(s);
  }
}

// -- Typed builder ------------------------------------------------------------
// Define a ChartDef in typed JS instead of (or alongside) the declarative JSON
// form. Both paths funnel through ChartDef.fromStates, so a built chart is
// behaviourally identical to the same chart parsed via ChartDef.fromChart.

/** A single transition, built in JS. Equivalent to a JSON transition object. */
export class TransitionBuilder {
  /** @param {string} target external transition target */
  constructor(target) {
    this._t = { target, guard: null, action: [], internal: false };
  }

  /** An external transition to `target` with no guard or actions. */
  static to(target) {
    return new TransitionBuilder(target);
  }

  /** Attach a named boolean guard (resolved at send time; absent → false). */
  guard(name) {
    this._t.guard = name;
    return this;
  }

  /** Append a transition action name. */
  action(name) {
    this._t.action.push(name);
    return this;
  }

  /** Mark this transition internal (no exit/re-entry when target is source or a descendant). */
  internal() {
    this._t.internal = true;
    return this;
  }
}

/** A single chart state, built in JS. Mirrors the JSON state object. */
export class StateBuilder {
  constructor(id, kind, { initial = null, history = null } = {}) {
    this.id = id;
    this._def = {
      id,
      parent: null,
      kind,
      history,
      initial,
      default: null,
      transitions: new Map(),
      entry: [],
      exit: [],
    };
  }

  /** An atomic leaf state. */
  static atomic(id) {
    return new StateBuilder(id, "atomic");
  }

  /** A compound state with the given initial child. */
  static compound(id, initial) {
    return new StateBuilder(id, "compound", { initial });
  }

  /** A parallel (orthogonal) state; all child regions are entered together. */
  static parallel(id) {
    return new StateBuilder(id, "parallel");
  }

  /** A final leaf state (accepted as a leaf; raises no completion event). */
  static final(id) {
    return new StateBuilder(id, "final");
  }

  /** A shallow-history pseudostate for its parent region. */
  static historyShallow(id) {
    return new StateBuilder(id, "history", { history: "shallow" });
  }

  /** A deep-history pseudostate for its parent region. */
  static historyDeep(id) {
    return new StateBuilder(id, "history", { history: "deep" });
  }

  /** Set the parent state id. Omit only for the single chart root. */
  parent(parent) {
    this._def.parent = parent;
    return this;
  }

  /** Set the default target used on a history pseudostate's first entry. */
  defaultChild(target) {
    this._def.default = target;
    return this;
  }

  /** Append an entry action name. */
  entry(action) {
    this._def.entry.push(action);
    return this;
  }

  /** Append an exit action name. */
  exit(action) {
    this._def.exit.push(action);
    return this;
  }

  /** Add an unguarded external transition on `event` to `target`. */
  on(event, target) {
    return this.onTransition(event, TransitionBuilder.to(target));
  }

  /** Add a guarded external transition on `event` to `target`. */
  onGuarded(event, target, guard) {
    return this.onTransition(event, TransitionBuilder.to(target).guard(guard));
  }

  /** Add a fully-specified transition on `event`. */
  onTransition(event, transitionBuilder) {
    this._def.transitions.set(event, transitionBuilder._t);
    return this;
  }
}

/**
 * Fluent builder assembling a {@link ChartDef} from typed JS states — the
 * definition path parallel to {@link ChartDef.fromChart}. State insertion order
 * fixes deterministic parallel-region descent, exactly as JSON key order does.
 */
export class ChartBuilder {
  constructor() {
    this._states = [];
  }

  /** Add a state. The first parent-less state added becomes the root. */
  state(stateBuilder) {
    this._states.push(stateBuilder);
    return this;
  }

  /**
   * Validate and assemble the {@link ChartDef}. Throws on a duplicate state id,
   * or on zero / more than one parent-less root, matching the JSON path.
   * @returns {ChartDef}
   */
  build() {
    const states = new Map();
    const order = new Map();
    let idx = 0;
    for (const sb of this._states) {
      if (states.has(sb.id)) {
        throw new TypeError(`duplicate state id \`${sb.id}\``);
      }
      order.set(sb.id, idx);
      idx += 1;
      states.set(sb.id, sb._def);
    }
    return ChartDef.fromStates(states, order);
  }
}
