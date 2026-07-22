// -- Lossless full-document tree CRDT — M1 syntax-agnostic core (#lzlosstree) --
//
// Native JS port of `lazily-rs::lossless_tree_crdt`. Where TextCrdt is a *flat*
// lossless floor and SeqCrdt orders opaque keyed siblings, this is a **single
// rooted concrete-syntax tree** whose *leaves own every rendered byte*. The
// guiding invariant is losslessness — `render(tree) === source_text` for valid,
// invalid, and unknown source alike — so the tree itself can be the wire
// authority instead of a semantic AST over a separate text floor. Internal
// element nodes own *structure only*; all text lives in leaf nodes tagged
// Token / Trivia / Raw / Error, so unknown/invalid spans round-trip exactly as
// Raw/Error leaves rather than being discarded.
//
// M1 scope: create / tombstone / intra-parent reorder / leaf-edit / split-leaf /
// merge-adjacent-leaves, plus op-based delta sync over a dotted, non-contiguous
// version frontier. Positions and seed text travel inside ops so both replicas
// store byte-identical keys and converge. Leaf text embeds TextCrdt wholesale;
// child order is a minimal fractional index (`keyBetween`, mirroring SeqCrdt);
// the clock is a Lamport TreeOpId. Leaf-local wire offsets are UTF-8 bytes,
// converted through utf8-offsets.

import { TextCrdt } from "./text-crdt.js";
import { byteToCodePoint } from "./utf8-offsets.js";

/** The four leaf classifications (PascalCase = the wire form). */
export const LeafKind = Object.freeze({
  Token: "Token",
  Trivia: "Trivia",
  Raw: "Raw",
  Error: "Error",
});

/** The sentinel id of the document root: {counter: 0, peer: 0}. */
export const ROOT = Object.freeze({ counter: 0, peer: 0 });

const idKey = (id) => `${id.counter}:${id.peer}`;
const cmpId = (a, b) => (a.counter !== b.counter ? a.counter - b.counter : a.peer - b.peer);
const minId = (a, b) => (cmpId(a, b) <= 0 ? a : b);

/** Lexicographic compare of two SortKeys (frac bytes, then peer). */
function cmpSort(a, b) {
  const n = Math.min(a.frac.length, b.frac.length);
  for (let i = 0; i < n; i++) {
    if (a.frac[i] !== b.frac[i]) return a.frac[i] - b.frac[i];
  }
  if (a.frac.length !== b.frac.length) return a.frac.length - b.frac.length;
  return a.peer - b.peer;
}

/**
 * A fractional key strictly between `lo` and `hi` (each `null` = open end),
 * compared lexicographically. Mirrors SeqCrdt's `key_between`; bytes are ints in
 * 0..255.
 */
export function keyBetween(lo, hi) {
  const result = [];
  let i = 0;
  const cap = (lo ? lo.length : 0) + (hi ? hi.length : 0) + 2;
  while (i <= cap) {
    const a = lo && i < lo.length ? lo[i] : 0;
    const b = hi ? (i < hi.length ? hi[i] : 0) : 256;
    if (a + 1 < b) {
      result.push(Math.floor((a + b) / 2));
      return result;
    }
    result.push(a);
    i += 1;
    if (a < b) {
      const loTail = lo && i <= lo.length ? lo.slice(i) : [];
      result.push(...keyBetween(loTail, null));
      return result;
    }
  }
  result.push(128);
  return result;
}

/** The observed dots for one peer: a contiguous prefix plus out-of-order holes. */
class DotRange {
  constructor() {
    this.contiguous = 0;
    this.sparse = new Set();
  }
  contains(counter) {
    return counter <= this.contiguous || this.sparse.has(counter);
  }
  observe(counter) {
    if (counter <= this.contiguous) return;
    this.sparse.add(counter);
    while (this.sparse.delete(this.contiguous + 1)) this.contiguous += 1;
  }
  copy() {
    const r = new DotRange();
    r.contiguous = this.contiguous;
    r.sparse = new Set(this.sparse);
    return r;
  }
}

/**
 * A dotted version frontier: per peer, exactly which op dots are held. Unlike a
 * version vector (per-peer max), this represents non-contiguous delivery so
 * `diff` never omits a missing interior op.
 */
export class TreeVersionFrontier {
  constructor() {
    this._dots = new Map();
  }
  contains(id) {
    const r = this._dots.get(id.peer);
    return r ? r.contains(id.counter) : false;
  }
  observe(id) {
    let r = this._dots.get(id.peer);
    if (!r) {
      r = new DotRange();
      this._dots.set(id.peer, r);
    }
    r.observe(id.counter);
  }
  copy() {
    const out = new TreeVersionFrontier();
    for (const [peer, r] of this._dots) out._dots.set(peer, r.copy());
    return out;
  }
}

/** A lossless concrete-syntax tree CRDT (M1 core). */
export class LosslessTreeCrdt {
  #peer;
  #counter;
  #nodes; // Map<idKey, NodeRecord>
  #frontier;
  #log;
  #buffered;

  constructor(peer) {
    this.#peer = peer;
    this.#counter = 0;
    this.#nodes = new Map();
    this.#nodes.set(idKey(ROOT), {
      parent: null,
      sort: { frac: [], peer: 0 },
      sortStamp: { counter: 0, peer: 0 },
      body: { kind: "element", elementKind: "root" },
      tomb: null,
      textHead: { counter: 0, peer: 0 },
    });
    this.#frontier = new TreeVersionFrontier();
    this.#log = [];
    this.#buffered = [];
  }

  /** Fork this replica's full state under a new owning `peer` (deep copy, new identity). */
  fork(peer) {
    const out = new LosslessTreeCrdt(peer);
    out.#counter = this.#counter;
    out.#nodes = new Map();
    for (const [k, r] of this.#nodes) {
      const body =
        r.body.kind === "leaf"
          ? { kind: "leaf", leafKind: r.body.leafKind, text: r.body.text.clone() }
          : { kind: "element", elementKind: r.body.elementKind };
      out.#nodes.set(k, { id: r.id, parent: r.parent, sort: r.sort, sortStamp: r.sortStamp, body, tomb: r.tomb, textHead: r.textHead });
    }
    out.#frontier = this.#frontier.copy();
    out.#log = this.#log.slice();
    out.#buffered = this.#buffered.slice();
    return out;
  }

  #nextOpId() {
    this.#counter += 1;
    return { counter: this.#counter, peer: this.#peer };
  }

  #get(id) {
    return this.#nodes.get(idKey(id));
  }

  /** Live children of `parent`, in rendered (SortKey) order. */
  #liveChildren(parent) {
    const pk = idKey(parent);
    const kids = [];
    for (const [, r] of this.#nodes) {
      if (r.parent && idKey(r.parent) === pk && r.tomb === null) kids.push(r);
    }
    kids.sort((a, b) => cmpSort(a.sort, b.sort));
    return kids.map((r) => r.id);
  }

  /** Render the whole document by concatenating live-leaf text in tree order. */
  render() {
    let out = "";
    const walk = (id) => {
      const r = this.#get(id);
      if (!r) return;
      if (r.body.kind === "leaf") out += r.body.text.text();
      else for (const child of this.#liveChildren(id)) walk(child);
    };
    walk(ROOT);
    return out;
  }

  /** Live nodes excluding the root — grows by one on split, restored on merge. */
  liveNodeCount() {
    let n = 0;
    const rootKey = idKey(ROOT);
    for (const [k, r] of this.#nodes) {
      if (k !== rootKey && r.tomb === null) n += 1;
    }
    return n;
  }

  /** This replica's dotted version frontier (what to advertise to a partner). */
  frontier() {
    return this.#frontier.copy();
  }

  /** The kind of an element node, or `null` if `node` is absent or a leaf. */
  elementKind(node) {
    const r = this.#get(node);
    return r && r.body.kind === "element" ? r.body.elementKind : null;
  }

  /** The kind of a leaf node, or `null` if `node` is absent or an element. */
  leafKind(node) {
    const r = this.#get(node);
    return r && r.body.kind === "leaf" ? r.body.leafKind : null;
  }

  /** Live children of `parent` in rendered order. */
  children(parent) {
    return this.#liveChildren(parent);
  }

  /** A leaf's current text; throws if `node` is absent or an element. */
  leafText(node) {
    const r = this.#get(node);
    if (!r) throw new Error("node not found");
    if (r.body.kind !== "leaf") throw new Error("node is not a leaf");
    return r.body.text.text();
  }

  #keyAfter(parent, after) {
    const order = this.#liveChildren(parent);
    let lo = null;
    let hi = null;
    if (after === null || after === undefined) {
      hi = order[0] ?? null;
    } else {
      const idx = order.findIndex((x) => idKey(x) === idKey(after));
      if (idx >= 0) {
        lo = after;
        hi = order[idx + 1] ?? null;
      } else {
        lo = order[order.length - 1] ?? null; // anchor gone: append at end
      }
    }
    const loFrac = lo ? this.#get(lo).sort.frac : null;
    const hiFrac = hi ? this.#get(hi).sort.frac : null;
    return { frac: keyBetween(loFrac, hiFrac), peer: this.#peer };
  }

  /** Create a node under `parent`, positioned after `after` (front when null). */
  createNode(parent, after, seed) {
    if (!this.#get(parent)) throw new Error("node not found");
    const sort = this.#keyAfter(parent, after);
    const opId = this.#nextOpId();
    const node = { counter: opId.counter, peer: opId.peer };
    this.#commitLocal({ id: opId, kind: { type: "CreateNode", id: node, parent, sort, seed } });
    return node;
  }

  /** Tombstone `node` (its subtree renders away once the ancestor is gone). */
  tombstoneNode(node) {
    if (!this.#get(node) || idKey(node) === idKey(ROOT)) throw new Error("node not found");
    const opId = this.#nextOpId();
    this.#commitLocal({ id: opId, kind: { type: "Tombstone", node } });
  }

  /** Reorder `node` within its parent to just after `after` (front when null). */
  reorderChild(node, after) {
    const rec = this.#get(node);
    if (!rec || !rec.parent) throw new Error("node not found");
    const sort = this.#keyAfter(rec.parent, after);
    const opId = this.#nextOpId();
    this.#commitLocal({ id: opId, kind: { type: "Reorder", node, sort } });
  }

  /**
   * Edit a leaf's text: delete `deleteBytes` and insert `insert` at UTF-8 byte
   * offset `atByte` (leaf-local). Offsets must land on char boundaries.
   */
  editLeaf(node, atByte, deleteBytes, insert) {
    const s = this.leafText(node);
    const start = byteToCodePoint(s, atByte);
    const end = byteToCodePoint(s, atByte + deleteBytes);
    if (start === null || end === null) throw new Error("offset not on a char boundary");
    const deleteCount = end - start;

    // Re-own the leaf's text under this replica so concurrent edits from
    // different peers mint distinct char ids (no collision on merge).
    const rec = this.#get(node);
    rec.body.text = rec.body.text.fork(this.#peer);
    const vv = rec.body.text.versionVector();
    for (let i = 0; i < deleteCount; i++) rec.body.text.delete(start);
    rec.body.text.insertStr(start, insert);
    const ops = rec.body.text.deltaSince(vv);

    const prev = rec.textHead;
    const opId = this.#nextOpId();
    this.#commitLocal({ id: opId, kind: { type: "LeafEdit", node, prev, ops } });
  }

  /**
   * Split a leaf at UTF-8 byte offset `atByte` into two adjacent leaves of the
   * same kind (head keeps `node`, tail is a fresh node returned here).
   */
  splitLeaf(node, atByte) {
    const s = this.leafText(node);
    const atChar = byteToCodePoint(s, atByte);
    if (atChar === null) throw new Error("offset not on a char boundary");
    const rec = this.#get(node);
    if (!rec.parent) throw new Error("node not found");
    const sort = this.#keyAfter(rec.parent, node);
    const prev = rec.textHead;
    const opId = this.#nextOpId();
    const newNode = { counter: opId.counter, peer: opId.peer };
    this.#commitLocal({ id: opId, kind: { type: "SplitLeaf", node, new: newNode, sort, atChar, prev } });
    return newNode;
  }

  /** Merge `right` into `left` when they are adjacent live leaf siblings. */
  mergeAdjacentLeaves(left, right) {
    this.leafText(left); // validate leaf-ness
    this.leafText(right);
    const rec = this.#get(left);
    if (!rec.parent) throw new Error("node not found");
    const order = this.#liveChildren(rec.parent);
    const li = order.findIndex((x) => idKey(x) === idKey(left));
    const adjacent = li >= 0 && order[li + 1] && idKey(order[li + 1]) === idKey(right);
    if (!adjacent) throw new Error("leaves are not adjacent live siblings");
    const prevLeft = this.#get(left).textHead;
    const prevRight = this.#get(right).textHead;
    const opId = this.#nextOpId();
    this.#commitLocal({ id: opId, kind: { type: "MergeLeaves", left, right, prevLeft, prevRight } });
  }

  /** Ops this replica holds that `their` frontier lacks, ordered by dotted id. */
  diff(their) {
    const ops = this.#log.filter((op) => !their.contains(op.id)).sort((a, b) => cmpId(a.id, b.id));
    return { ops };
  }

  /**
   * Apply a batch of remote ops. Idempotent (already-held ops skipped) and
   * order-tolerant (an op whose target/parent has not arrived is buffered and
   * retried). Advances the Lamport counter past every observed op.
   */
  applyUpdate(update) {
    for (const op of update.ops) {
      this.#counter = Math.max(this.#counter, op.id.counter);
      if (this.#frontier.contains(op.id)) continue;
      this.#buffered.push(op);
    }
    this.#drainBuffered();
  }

  #drainBuffered() {
    for (;;) {
      let progressed = false;
      const pending = this.#buffered;
      this.#buffered = [];
      for (const op of pending) {
        if (this.#frontier.contains(op.id)) continue;
        if (this.#dependenciesReady(op)) {
          this.#applyOp(op);
          this.#record(op);
          progressed = true;
        } else {
          this.#buffered.push(op);
        }
      }
      if (!progressed) break;
    }
  }

  #dependenciesReady(op) {
    const k = op.kind;
    switch (k.type) {
      case "CreateNode":
        return !!this.#get(k.parent);
      case "Tombstone":
      case "Reorder":
        return !!this.#get(k.node);
      case "LeafEdit":
      case "SplitLeaf":
        return !!this.#get(k.node) && this.#frontier.contains(k.prev);
      case "MergeLeaves":
        return (
          !!this.#get(k.left) &&
          !!this.#get(k.right) &&
          this.#frontier.contains(k.prevLeft) &&
          this.#frontier.contains(k.prevRight)
        );
      default:
        return false;
    }
  }

  #commitLocal(op) {
    this.#applyOp(op);
    this.#record(op);
  }

  #record(op) {
    this.#frontier.observe(op.id);
    this.#log.push(op);
  }

  #applyOp(op) {
    const k = op.kind;
    switch (k.type) {
      case "CreateNode": {
        if (this.#get(k.id)) return;
        const body =
          k.seed.type === "leaf"
            ? { kind: "leaf", leafKind: k.seed.leafKind, text: TextCrdt.fromStr(k.id.peer, k.seed.text) }
            : { kind: "element", elementKind: k.seed.kind };
        this.#nodes.set(idKey(k.id), {
          id: k.id,
          parent: k.parent,
          sort: k.sort,
          sortStamp: op.id,
          body,
          tomb: null,
          textHead: op.id,
        });
        break;
      }
      case "Tombstone": {
        const rec = this.#get(k.node);
        if (rec) rec.tomb = rec.tomb ? minId(rec.tomb, op.id) : op.id;
        break;
      }
      case "Reorder": {
        const rec = this.#get(k.node);
        if (rec && cmpId(op.id, rec.sortStamp) > 0) {
          rec.sort = k.sort;
          rec.sortStamp = op.id;
        }
        break;
      }
      case "LeafEdit": {
        const rec = this.#get(k.node);
        if (rec && rec.body.kind === "leaf") {
          rec.body.text.applyDelta(k.ops);
          rec.textHead = op.id;
        }
        break;
      }
      case "SplitLeaf":
        this.#applySplit(k.node, k.new, k.sort, k.atChar, op.id);
        break;
      case "MergeLeaves":
        this.#applyMerge(k.left, k.right, op.id);
        break;
    }
  }

  #applySplit(node, newNode, sort, atChar, opId) {
    const rec = this.#get(node);
    if (!rec || rec.body.kind !== "leaf") return;
    const leafKind = rec.body.leafKind;
    const parent = rec.parent;
    const cps = [...rec.body.text.text()]; // code-point array
    const clamp = Math.min(atChar, cps.length);
    const head = cps.slice(0, clamp).join("");
    const tail = cps.slice(clamp).join("");
    // Reseed head under the original node's create peer so both replicas rebuild
    // byte-identical leaf state.
    rec.body = { kind: "leaf", leafKind, text: TextCrdt.fromStr(node.peer, head) };
    rec.textHead = opId;
    if (!this.#get(newNode)) {
      this.#nodes.set(idKey(newNode), {
        id: newNode,
        parent,
        sort,
        sortStamp: opId,
        body: { kind: "leaf", leafKind, text: TextCrdt.fromStr(newNode.peer, tail) },
        tomb: null,
        textHead: opId,
      });
    }
  }

  #applyMerge(left, right, opId) {
    const l = this.#get(left);
    const r = this.#get(right);
    if (!l || !r || l.body.kind !== "leaf" || r.body.kind !== "leaf") return;
    const combined = l.body.text.text() + r.body.text.text();
    l.body = { kind: "leaf", leafKind: l.body.leafKind, text: TextCrdt.fromStr(left.peer, combined) };
    l.textHead = opId;
    r.tomb = r.tomb ? minId(r.tomb, opId) : opId;
  }
}

/**
 * Serialize a TreeUpdate (the internal `diff`/log form) to the externally-tagged
 * wire JSON that validates against lazily-spec's lossless-tree-delta.json. Ops,
 * seeds, and node ids all become the schema's normative shapes.
 */
export function treeUpdateToWire(update) {
  const seedWire = (seed) =>
    seed.type === "leaf"
      ? { Leaf: { kind: seed.leafKind, text: seed.text } }
      : { Element: { kind: seed.kind } };
  const kindWire = (k) => {
    switch (k.type) {
      case "CreateNode":
        return { CreateNode: { id: k.id, parent: k.parent, sort: k.sort, seed: seedWire(k.seed) } };
      case "Tombstone":
        return { Tombstone: { node: k.node } };
      case "Reorder":
        return { Reorder: { node: k.node, sort: k.sort } };
      case "LeafEdit":
        return { LeafEdit: { node: k.node, prev: k.prev, ops: k.ops } };
      case "SplitLeaf":
        return { SplitLeaf: { node: k.node, new: k.new, sort: k.sort, at_char: k.atChar, prev: k.prev } };
      case "MergeLeaves":
        return { MergeLeaves: { left: k.left, right: k.right, prev_left: k.prevLeft, prev_right: k.prevRight } };
      default:
        throw new Error(`unknown op kind: ${k.type}`);
    }
  };
  return { ops: update.ops.map((op) => ({ id: op.id, kind: kindWire(op.kind) })) };
}
