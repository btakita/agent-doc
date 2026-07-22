// Memoized semantic tree (cell-model.md § Memoized semantic tree, #lzsemtree).
// One memo slot per node folds (node value, child derived values). Editing one
// node recomputes only its ANCESTOR CHAIN — a sibling subtree's derived slot
// stays cached (incremental, glitch-free). A node edit that does not change
// the folded result MUST NOT re-run a downstream consumer (memo equality guard).
// Cost is proportional to the diff, not the document.
//
// The tree is reactive: each node's value is a `Cell` and its child list is a
// `Cell`, so a SemTree composes over the owning `Context`. Structural growth
// (inserting a brand-new child) requires `build` again (the captured child-slot
// map is fixed at build time, per the lazily-rs/lazily-kt contract); removals
// are covered by incrementality because the parent's child-list cell changes.

import { Context } from "./reactive.js";

class SemNode {
  constructor(id, value, ctx) {
    this.id = id;
    this.valueCell = ctx.source(value);
    this.childKeysCell = ctx.source([]); // array of child ids (reactive membership/order)
    this.childSlots = new Map(); // child id -> Computed (captured at build)
    this.slot = null; // guarded Computed for this node's derived value
  }
}

export class SemTree {
  #ctx;
  #nodes = new Map(); // id -> SemNode
  #rootId;
  #fold;

  constructor(ctx, rootSpec, fold) {
    this.#ctx = ctx;
    this.#fold = fold;
    this.#rootId = rootSpec.id;
    this.#build(rootSpec);
  }

  static build(ctx, rootSpec, fold) {
    return new SemTree(ctx, rootSpec, fold);
  }

  #build(spec) {
    const node = new SemNode(spec.id, spec.value, this.#ctx);
    this.#nodes.set(spec.id, node);
    const childOrder = [];
    const children = (spec.children && spec.children.values) ?? {};
    const order = (spec.children && spec.children.order) ?? Object.keys(children);
    for (const childKey of order) {
      const childSpec = children[childKey];
      if (!childSpec) {
        continue;
      }
      const childNode = this.#build(childSpec);
      childOrder.push(childSpec.id);
      node.childSlots.set(childSpec.id, childNode.slot);
    }
    // Set the child-list cell BEFORE registering the memo so the memo observes it.
    this.#ctx.set(node.childKeysCell, childOrder);
    // Register the memo slot: subscribes to own value cell, own child-list cell,
    // and each present child's derived slot.
    const ctx = this.#ctx;
    const fold = this.#fold;
    const self = this;
    node.slot = ctx.computed((c) => {
      const v = c.get(node.valueCell);
      const kids = c.get(node.childKeysCell);
      const ds = [];
      for (const kid of kids) {
        const childSlot = node.childSlots.get(kid);
        if (childSlot) {
          ds.push(c.get(childSlot));
        }
      }
      return fold(v, ds);
    });
    return node;
  }

  // Edit a node's value cell. Only the ancestor chain recomputes.
  setValue(id, value) {
    const node = this.#nodes.get(id);
    if (!node) {
      throw new RangeError(`SemTree node not present: ${String(id)}`);
    }
    this.#ctx.set(node.valueCell, value);
  }

  // Remove a child from its parent's child-list cell. The parent re-folds over
  // the remaining children (covered by incrementality).
  removeChild(parentId, childId) {
    const parent = this.#nodes.get(parentId);
    if (!parent) {
      throw new RangeError(`SemTree parent not present: ${String(parentId)}`);
    }
    const kids = this.#ctx.get(parent.childKeysCell).filter((k) => k !== childId);
    this.#ctx.set(parent.childKeysCell, kids);
  }

  // Reactive read of the root derived value.
  value() {
    return this.#ctx.get(this.#nodes.get(this.#rootId).slot);
  }

  // Reactive read at a node id.
  nodeValue(id) {
    const node = this.#nodes.get(id);
    if (!node) {
      return undefined;
    }
    return this.#ctx.get(node.slot);
  }

  // Whether a node's derived slot currently has a fresh cached value (testing).
  isCached(id) {
    const node = this.#nodes.get(id);
    if (!node) {
      return false;
    }
    return this.#ctx.isSet(node.slot);
  }

  rootHandle() {
    return this.#nodes.get(this.#rootId).slot;
  }

  nodeHandle(id) {
    return this.#nodes.get(id)?.slot ?? null;
  }
}
