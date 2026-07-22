// Keyed cell collections: CellMap + LIS keyed reconciliation (cell-model.md
// § Keyed cell collections). Pure logic — no reactive graph. Like the state
// chart, this is compute that every binding (including this state-projection
// consumer) MUST implement; the conformance/collections/ fixtures pin behavior.
//
// Invalidation classes mirror the reactive independence contract: a value write
// touches only value readers of that key; an insert/remove touches membership +
// order readers; a pure reorder (atomic move) touches only order readers and
// keeps the entry's stable handle (never remove + re-mint).

function deepEqual(a, b) {
  if (a === b) {
    return true;
  }
  if (a === null || b === null || typeof a !== "object" || typeof b !== "object") {
    return false;
  }
  // #lzjsshalloweq: fast path for plain arrays — Array.isArray + length check
  // before Object.keys, index loop, no closure allocation.
  if (Array.isArray(a) && Array.isArray(b)) {
    const n = a.length;
    if (n !== b.length) {
      return false;
    }
    for (let i = 0; i < n; i++) {
      if (!deepEqual(a[i], b[i])) {
        return false;
      }
    }
    return true;
  }
  const aKeys = Object.keys(a);
  const bKeys = Object.keys(b);
  if (aKeys.length !== bKeys.length) {
    return false;
  }
  for (let i = 0; i < aKeys.length; i++) {
    const k = aKeys[i];
    if (!Object.is(aKeys[k], bKeys[k]) || !deepEqual(a[k], b[k])) {
      return false;
    }
  }
  return true;
}

function indexOfKey(order, key) {
  const index = order.indexOf(key);
  if (index === -1) {
    throw new RangeError(`CellMap key not present: ${String(key)}`);
  }
  return index;
}

// Longest strictly-increasing subsequence, returned as indices into `seq`.
// Classic patience-sorting reconstruction (O(n log n)); ties break toward the
// earliest equal value so the stable set is the longest possible.
function longestIncreasingSubsequence(seq) {
  const n = seq.length;
  if (n === 0) {
    return [];
  }
  const tails = []; // tails[i] = index (into seq) of the smallest tail of an IS of length i+1
  const prev = new Array(n).fill(-1);
  for (let i = 0; i < n; i++) {
    const value = seq[i];
    let lo = 0;
    let hi = tails.length;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      if (seq[tails[mid]] < value) {
        lo = mid + 1;
      } else {
        hi = mid;
      }
    }
    if (lo > 0) {
      prev[i] = tails[lo - 1];
    }
    tails[lo] = i;
  }
  const lis = [];
  for (let cursor = tails[tails.length - 1]; cursor !== -1; cursor = prev[cursor]) {
    lis.push(cursor);
  }
  return lis.reverse();
}

export class CellMap {
  constructor(initial = {}) {
    const order = Array.isArray(initial.order) ? [...initial.order] : [];
    const values = initial.values ?? {};
    this.order = order;
    this.values = new Map();
    this.handles = new Map();
    this.#nextHandle = 0;
    for (const key of order) {
      this.values.set(key, values[key]);
      this.handles.set(key, this.#nextHandle++);
    }
    Object.freeze(this);
  }

  #nextHandle;

  static from(initial) {
    return new CellMap(initial);
  }

  keys() {
    return [...this.order];
  }

  has(key) {
    return this.handles.has(key);
  }

  get(key) {
    return this.values.get(key);
  }

  handle(key) {
    return this.handles.get(key);
  }

  snapshot() {
    const values = {};
    for (const [key, value] of this.values) {
      values[key] = value;
    }
    return { order: [...this.order], values };
  }

  // Dispatch a fixture op; returns the invalidation report
  // { value: key[], membership: bool, order: bool }.
  apply(op) {
    switch (op.type) {
      case "set_value":
        return this.setValue(op.key, op.value);
      case "insert":
        return this.insert(op.key, op.value, op.at);
      case "remove":
        return this.remove(op.key);
      case "move_to":
        return this.moveTo(op.key, op.index);
      case "move_before":
        return this.moveBefore(op.key, op.before);
      case "move_after":
        return this.moveAfter(op.key, op.after);
      default:
        throw new TypeError(`unknown CellMap op type: ${op.type}`);
    }
  }

  setValue(key, value) {
    if (!this.handles.has(key)) {
      // No fixture exercises this; treat as a value-less insert to stay total.
      return this.insert(key, value, "end");
    }
    const changed = !deepEqual(this.values.get(key), value);
    if (changed) {
      this.values.set(key, value);
    }
    return { value: changed ? [key] : [], membership: false, order: false };
  }

  insert(key, value, at = "end") {
    if (this.handles.has(key)) {
      throw new RangeError(`CellMap key already present: ${String(key)}`);
    }
    let index;
    if (at === "end") {
      index = this.order.length;
    } else if (at === "start") {
      index = 0;
    } else if (typeof at === "number") {
      index = Math.max(0, Math.min(at, this.order.length));
    } else {
      index = indexOfKey(this.order, at) + 1; // anchor key → insert after it
    }
    this.order.splice(index, 0, key);
    this.values.set(key, value);
    this.handles.set(key, this.#nextHandle++);
    return { value: [], membership: true, order: true };
  }

  remove(key) {
    const index = indexOfKey(this.order, key);
    this.order.splice(index, 1);
    this.values.delete(key);
    this.handles.delete(key);
    return { value: [], membership: true, order: true };
  }

  moveTo(key, index) {
    const from = indexOfKey(this.order, key);
    const to = Math.max(0, Math.min(index, this.order.length - 1));
    if (from !== to) {
      this.order.splice(from, 1);
      this.order.splice(to, 0, key);
    }
    return { value: [], membership: false, order: true };
  }

  moveBefore(key, beforeKey) {
    return this.#reorderBefore(key, indexOfKey(this.order, beforeKey));
  }

  moveAfter(key, afterKey) {
    return this.#reorderBefore(key, indexOfKey(this.order, afterKey) + 1);
  }

  #reorderBefore(key, targetSlot) {
    const from = indexOfKey(this.order, key);
    // Remove first, then compute the slot in the shortened order so an anchor
    // before the moved key does not shift past it.
    this.order.splice(from, 1);
    let slot = targetSlot;
    if (from < targetSlot) {
      slot = targetSlot - 1;
    }
    slot = Math.max(0, Math.min(slot, this.order.length));
    this.order.splice(slot, 0, key);
    // Handle is untouched — a move is never a remove + re-mint.
    return { value: [], membership: false, order: true };
  }
}

// Standalone keyed reconciliation (cell-model.md § Keyed reconciliation).
// Diffs two keyed sequences BY STABLE KEY, not position, emitting the minimal
// {remove, move, insert} op set. Keys already in relative order (the longest
// increasing subsequence over their prior indices) MUST NOT move, and stable
// entries with unchanged values are not invalidated by a sibling reorder.
export function reconcileCollections(prior, target) {
  const priorOrder = Array.isArray(prior.order) ? [...prior.order] : [];
  const priorValues = prior.values ?? {};
  const targetOrder = Array.isArray(target.order) ? [...target.order] : [];
  const targetValues = target.values ?? {};

  const priorIndex = new Map(priorOrder.map((key, i) => [key, i]));
  const targetSet = new Set(targetOrder);

  const commonInTarget = targetOrder.filter((key) => priorIndex.has(key));
  const priorIndices = commonInTarget.map((key) => priorIndex.get(key));
  const lisPositions = new Set(longestIncreasingSubsequence(priorIndices));
  const stableKeys = new Set(
    commonInTarget.filter((_, i) => lisPositions.has(i)),
  );

  // Stable entries whose value is also unchanged keep their value cell intact.
  const stableKeysNotInvalidated = commonInTarget.filter(
    (key) =>
      stableKeys.has(key) && deepEqual(priorValues[key], targetValues[key]),
  );

  const ops = [];
  // 1. removals (prior-only keys), in prior order.
  for (const key of priorOrder) {
    if (!targetSet.has(key)) {
      ops.push({ type: "remove", key });
    }
  }
  // 2. moves + inserts, walking the target order; each is anchored after the
  //    previously placed key (stable entries are already placed).
  let placedAfter = null;
  for (const key of targetOrder) {
    if (stableKeys.has(key)) {
      placedAfter = key;
      continue;
    }
    if (priorIndex.has(key)) {
      ops.push({ type: "move", key, after: placedAfter });
    } else {
      ops.push({ type: "insert", key, value: targetValues[key], after: placedAfter });
    }
    placedAfter = key;
  }

  return {
    ops,
    result_order: [...targetOrder],
    stable_keys_not_invalidated: stableKeysNotInvalidated,
  };
}

// Ordered keyed tree (cell-model.md § Ordered keyed tree). A `CellTree` is a
// further **composition**: each node is `(stable id, value, ordered keyed child
// collection)` — the child collection is a `CellMap` whose values are child
// tree nodes. Per-node value reactivity, per-level membership/order
// reactivity, and the atomic-move guarantee are all inherited from the per-cell
// model: editing a node touches only that node's value; adding/removing/reordering
// siblings touches only that parent's child level; a child reorder keeps the
// entry's stable handle and bumps order once (never remove + re-mint).
//
// Like `CellMap`, this is pure logic with no reactive graph. Each mutating op
// returns an invalidation report scoped to the **affected path only** — a
// reader at any other path observes no change, which is what makes the
// per-level independence invariant observable.
function buildNode(spec) {
  const rawChildren = (spec && spec.children) || {};
  const order = Array.isArray(rawChildren.order) ? [...rawChildren.order] : [];
  const childSpecs = rawChildren.values ?? {};
  const childNodes = {};
  for (const key of order) {
    childNodes[key] = buildNode(childSpecs[key]);
  }
  return new TreeNode(spec && spec.id, spec && spec.value, new CellMap({ order, values: childNodes }));
}

export class TreeNode {
  constructor(id, value, children) {
    this.id = id;
    this.value = value;
    this.children = children;
  }

  snapshot() {
    const childOrder = this.children.order;
    const childValues = {};
    for (const key of childOrder) {
      childValues[key] = this.children.get(key).snapshot();
    }
    return { id: this.id, value: this.value, children: { order: [...childOrder], values: childValues } };
  }
}

export class CellTree {
  constructor(rootSpec) {
    this.root = rootSpec instanceof TreeNode ? rootSpec : buildNode(rootSpec);
    Object.freeze(this);
  }

  static from(rootSpec) {
    return new CellTree(rootSpec);
  }

  // Resolve a path (array of child keys from the root) to a node, or undefined.
  nodeAt(path) {
    const keys = Array.isArray(path) ? path : [path];
    let node = this.root;
    for (const key of keys) {
      const child = node.children.get(key);
      if (child === undefined) {
        return undefined;
      }
      node = child;
    }
    return node;
  }

  #nodeAtOrThrow(path) {
    const node = this.nodeAt(path);
    if (node === undefined) {
      throw new RangeError(`CellTree path not present: ${JSON.stringify(path)}`);
    }
    return node;
  }

  #parentAndKey(path) {
    const keys = Array.isArray(path) ? path : [path];
    if (keys.length === 0) {
      throw new RangeError("CellTree path must have at least one key");
    }
    const parentPath = keys.slice(0, -1);
    const key = keys[keys.length - 1];
    const parent = parentPath.length === 0 ? this.root : this.#nodeAtOrThrow(parentPath);
    return { parent, key, parentPath };
  }

  getValue(path) {
    return this.#nodeAtOrThrow(path).value;
  }

  setValue(path, value) {
    const { parent, key } = this.#parentAndKey(path);
    const node = parent.children.get(key);
    if (node === undefined) {
      throw new RangeError(`CellTree key not present: ${String(key)}`);
    }
    const changed = !deepEqual(node.value, value);
    if (changed) {
      node.value = value;
    }
    return { path: Array.isArray(path) ? path : [path], value: changed ? [key] : [], membership: false, order: false };
  }

  hasChild(path, key) {
    return this.#nodeAtOrThrow(path).children.has(key);
  }

  childKeys(path) {
    return this.#nodeAtOrThrow(path).children.keys();
  }

  childHandle(path, key) {
    return this.#nodeAtOrThrow(path).children.handle(key);
  }

  insertChild(path, key, childSpec, at = "end") {
    const parent = this.#nodeAtOrThrow(path);
    const node = childSpec instanceof TreeNode ? childSpec : buildNode(childSpec);
    parent.children.insert(key, node, at);
    return { path: Array.isArray(path) ? path : [path], value: [], membership: true, order: true };
  }

  removeChild(path, key) {
    const parent = this.#nodeAtOrThrow(path);
    parent.children.remove(key);
    return { path: Array.isArray(path) ? path : [path], value: [], membership: true, order: true };
  }

  moveChildTo(path, key, index) {
    const parent = this.#nodeAtOrThrow(path);
    parent.children.moveTo(key, index);
    return { path: Array.isArray(path) ? path : [path], value: [], membership: false, order: true };
  }

  moveChildBefore(path, key, beforeKey) {
    const parent = this.#nodeAtOrThrow(path);
    parent.children.moveBefore(key, beforeKey);
    return { path: Array.isArray(path) ? path : [path], value: [], membership: false, order: true };
  }

  moveChildAfter(path, key, afterKey) {
    const parent = this.#nodeAtOrThrow(path);
    parent.children.moveAfter(key, afterKey);
    return { path: Array.isArray(path) ? path : [path], value: [], membership: false, order: true };
  }

  snapshot() {
    return this.root.snapshot();
  }
}
