// Fugue/RGA-style character CRDT (cell-model.md § Free-text CRDT + re-parse,
// #lztextcrdt). Each character is an element with a unique `OpId` (counter,
// peer) and a left-origin (`origin`); deletes are sticky tombstones carrying
// the delete op's own id. The visible sequence order is a pure, deterministic
// function of the element set: a pre-order DFS of the origin tree where
// siblings of the same origin are visited in DESCENDING `OpId` order
// (most-recent first). Merge is commutative, associative, idempotent;
// concurrent same-point inserts keep both, ordered by the peer tiebreak.

// Element identity = (counter, peer). Total order is (counter, peer) ascending.
export class OpId {
  constructor(counter, peer) {
    this.counter = counter;
    this.peer = peer;
    // Cached string key — avoids per-lookup allocation across the lifetime of
    // an OpId (#lzopidkeytuple). Set before freeze so the field is immutable
    // alongside the rest of the instance.
    this._key = `${counter}:${peer}`;
    Object.freeze(this);
  }
  compareTo(other) {
    if (this.counter !== other.counter) {
      return this.counter < other.counter ? -1 : 1;
    }
    if (this.peer !== other.peer) {
      return this.peer < other.peer ? -1 : 1;
    }
    return 0;
  }
  static desc(a, b) {
    return b.compareTo(a); // descending
  }
}

class Elem {
  // `id` is stored on the Elem so iteration over the map values can recover
  // the OpId without re-parsing the string key (#lzopidkeytuple).
  constructor(id, ch, origin, deleted = null) {
    this.id = id; // OpId
    this.ch = ch;
    this.origin = origin; // OpId | null (null = document start)
    this.deleted = deleted; // OpId | null (the delete op id, or null = live)
  }
}

export class TextCrdt {
  // String-keyed map (OpId._key -> Elem). The string is cached on each OpId at
  // construction so this is a hash lookup with zero per-op allocation.
  #elems = new Map();
  #peer;
  #counter = 0;
  // Cached visible orderings (#lztextordcache). Both are invalidated by every
  // mutation; populated lazily on the next read. Repeated `text()` between
  // mutations is O(1) instead of O(N log N).
  #orderedLiveCache = null; // OpId[] | null (excludes tombstones)
  #orderedAllCache = null; // OpId[] | null (includes tombstones)

  constructor(peer) {
    this.#peer = peer;
  }

  static fromStr(peer, str) {
    const crdt = new TextCrdt(peer);
    crdt.insertStr(0, str);
    return crdt;
  }

  // Deep-copy elems, adopt a new peer, COPY the counter (so ids stay unique).
  fork(peer) {
    const copy = new TextCrdt(peer);
    copy.#counter = this.#counter;
    for (const elem of this.#elems.values()) {
      const e = new Elem(elem.id, elem.ch, elem.origin, elem.deleted);
      copy.#elems.set(e.id._key, e);
    }
    return copy;
  }

  clone() {
    return this.fork(this.#peer);
  }

  #nextId() {
    this.#counter += 1; // pre-increment: first id has counter == 1
    return new OpId(this.#counter, this.#peer);
  }

  #invalidateOrdered() {
    this.#orderedLiveCache = null;
    this.#orderedAllCache = null;
  }

  #orderedIds(includeDeleted) {
    // Cache hit (#lztextordcache).
    if (includeDeleted) {
      if (this.#orderedAllCache !== null) return this.#orderedAllCache;
    } else {
      if (this.#orderedLiveCache !== null) return this.#orderedLiveCache;
    }

    // Group by origin. Keys are origin._key (or "<root>"); values are OpId[]
    // recovered directly from Elem.id — no string parse (#lzopidkeytuple).
    const children = new Map();
    for (const elem of this.#elems.values()) {
      const originKey = elem.origin ? elem.origin._key : "<root>";
      let list = children.get(originKey);
      if (!list) {
        list = [];
        children.set(originKey, list);
      }
      list.push(elem.id);
    }
    // Sort each sibling list DESCENDING by OpId.
    for (const list of children.values()) {
      list.sort(OpId.desc);
    }
    // Iterative pre-order DFS.
    const out = [];
    const roots = children.get("<root>") ?? [];
    const stack = [...roots].sort(OpId.desc); // highest pops first
    while (stack.length > 0) {
      const id = stack.pop();
      const elem = this.#elems.get(id._key);
      if (!elem) {
        continue;
      }
      if (includeDeleted || elem.deleted === null) {
        out.push(id);
      }
      const kids = children.get(id._key);
      if (kids) {
        // Push reversed so the highest-OpId child pops first.
        for (let i = kids.length - 1; i >= 0; i--) {
          stack.push(kids[i]);
        }
      }
    }
    if (includeDeleted) {
      this.#orderedAllCache = out;
    } else {
      this.#orderedLiveCache = out;
    }
    return out;
  }

  insert(index, ch) {
    const visible = this.#orderedIds(false);
    const origin = index === 0 ? null : visible[index - 1] ?? null;
    const id = this.#nextId();
    this.#invalidateOrdered();
    this.#elems.set(id._key, new Elem(id, ch, origin, null));
  }

  // Bulk insert with origin chaining (#lztextinsertchain): one `orderedIds()`
  // pass + N chain appends instead of N full-tree rebuilds. Sequential chars
  // chain naturally — char i+1's left-origin is char i's just-minted OpId —
  // so the DFS visits them in chain order (counter strictly increases under
  // one peer). Concurrent inserts at the same point still sort by the peer
  // tiebreak, preserving the standard CRDT convergence contract.
  insertStr(index, str) {
    const visible = this.#orderedIds(false);
    let origin = index === 0 ? null : visible[index - 1] ?? null;
    this.#invalidateOrdered();
    for (const ch of String(str)) {
      const id = this.#nextId();
      this.#elems.set(id._key, new Elem(id, ch, origin, null));
      origin = id; // chain: next char's left-origin is this id
    }
  }

  delete(index) {
    const visible = this.#orderedIds(false);
    const id = visible[index];
    if (id === undefined) {
      return; // no-op if out of range
    }
    const del = this.#nextId(); // always advance the clock (matches lazily-rs)
    const elem = this.#elems.get(id._key);
    if (elem && elem.deleted === null) {
      this.#invalidateOrdered();
      elem.deleted = del;
    }
  }

  text() {
    const ordered = this.#orderedIds(false);
    let out = "";
    for (const id of ordered) {
      out += this.#elems.get(id._key).ch;
    }
    return out;
  }

  // Count of live (non-tombstoned) elems over the whole map.
  len() {
    let count = 0;
    for (const elem of this.#elems.values()) {
      if (elem.deleted === null) {
        count++;
      }
    }
    return count;
  }

  is_empty() {
    return this.len() === 0;
  }

  tombstoneCount() {
    let count = 0;
    for (const elem of this.#elems.values()) {
      if (elem.deleted !== null) {
        count++;
      }
    }
    return count;
  }

  clock() {
    return new OpId(this.#counter, this.#peer);
  }

  // State-based merge (whole-replica input). Commutative, associative,
  // idempotent. Tombstones are sticky: any Some wins; concurrent deletes take
  // the smaller delete id. Returns whether visible text changed.
  merge(other) {
    const before = this.text();
    let anyChange = false;
    for (const oe of other.#elems.values()) {
      this.#counter = Math.max(this.#counter, oe.id.counter);
      if (oe.deleted) {
        this.#counter = Math.max(this.#counter, oe.deleted.counter);
      }
      const existing = this.#elems.get(oe.id._key);
      if (existing) {
        if (existing.deleted && oe.deleted) {
          const merged =
            existing.deleted.compareTo(oe.deleted) <= 0
              ? existing.deleted
              : oe.deleted;
          if (merged !== existing.deleted) {
            existing.deleted = merged;
            anyChange = true;
          }
        } else if (oe.deleted && existing.deleted === null) {
          existing.deleted = oe.deleted;
          anyChange = true;
        }
      } else {
        // Adopt the elem with its existing OpId identity so later concurrent
        // edits merge without duplication.
        const adopted = new Elem(oe.id, oe.ch, oe.origin, oe.deleted);
        this.#elems.set(oe.id._key, adopted);
        anyChange = true;
      }
    }
    if (anyChange) {
      this.#invalidateOrdered();
    }
    return this.text() !== before;
  }

  // CrdtTree materialized-value surface.
  value() {
    return this.text();
  }

  // CrdtTree state-join surface.
  mergeFrom(other) {
    return this.merge(other);
  }

  // Tombstone GC: collect a stable deleted element only when nothing references
  // it as a left origin. Bottom-up: pass 1 collects unreferenced leaf
  // tombstones, removing a leaf un-references its origin, so further passes
  // collect interior tombstones until fixpoint. `isStable` is invoked on the
  // DELETE op id (caller-supplied causal-stability policy).
  gcWith(isStable) {
    let removed = 0;
    while (true) {
      const referenced = new Set();
      for (const elem of this.#elems.values()) {
        if (elem.origin) {
          referenced.add(elem.origin._key);
        }
      }
      const collectable = [];
      for (const [key, elem] of this.#elems) {
        if (elem.deleted !== null && isStable(elem.deleted) && !referenced.has(key)) {
          collectable.push(key);
        }
      }
      if (collectable.length === 0) {
        break;
      }
      for (const key of collectable) {
        this.#elems.delete(key);
        removed++;
      }
    }
    if (removed > 0) {
      this.#invalidateOrdered();
    }
    return removed;
  }

  // --- Delta sync (#lztextsync) ---

  // Version vector: {peer -> greatest counter seen} over inserts + deletes. The
  // compact frontier a replica sends so a partner can compute the ops it lacks.
  versionVector() {
    const vv = {};
    for (const elem of this.#elems.values()) {
      const id = elem.id;
      vv[id.peer] = Math.max(vv[id.peer] ?? 0, id.counter);
      if (elem.deleted) {
        vv[elem.deleted.peer] = Math.max(vv[elem.deleted.peer] ?? 0, elem.deleted.counter);
      }
    }
    return vv;
  }

  // The ops `theirVv` has not observed — new inserts + newly-observed tombstones.
  // Each TextOp is a plain {id, ch, origin, deleted} of {counter, peer} ids. A
  // whole-state snapshot is `deltaSince({})`.
  deltaSince(theirVv) {
    const seen = (id) => id.counter <= (theirVv[id.peer] ?? 0);
    const wire = (id) => (id ? { counter: id.counter, peer: id.peer } : null);
    const out = [];
    for (const elem of this.#elems.values()) {
      const id = elem.id;
      const insertNew = !seen(id);
      const deleteNew = elem.deleted !== null && !seen(elem.deleted);
      if (insertNew || deleteNew) {
        out.push({
          id: wire(id),
          ch: elem.ch,
          origin: wire(elem.origin),
          deleted: wire(elem.deleted),
        });
      }
    }
    return out;
  }

  // Apply a delta op list (from `deltaSince`). Commutative, associative,
  // idempotent — the same convergence contract as `merge`, from the transport
  // form. Rebuilding a replica via `applyDelta` preserves OpId identity so later
  // concurrent edits merge without duplication. Returns whether text changed.
  applyDelta(ops) {
    const before = this.text();
    let anyChange = false;
    for (const op of ops) {
      const id = new OpId(op.id.counter, op.id.peer);
      const origin = op.origin ? new OpId(op.origin.counter, op.origin.peer) : null;
      const deleted = op.deleted ? new OpId(op.deleted.counter, op.deleted.peer) : null;
      this.#counter = Math.max(this.#counter, id.counter);
      if (deleted) {
        this.#counter = Math.max(this.#counter, deleted.counter);
      }
      const existing = this.#elems.get(id._key);
      if (existing) {
        if (existing.deleted && deleted) {
          const merged =
            existing.deleted.compareTo(deleted) <= 0 ? existing.deleted : deleted;
          if (merged !== existing.deleted) {
            existing.deleted = merged;
            anyChange = true;
          }
        } else if (deleted && existing.deleted === null) {
          existing.deleted = deleted;
          anyChange = true;
        }
      } else {
        this.#elems.set(id._key, new Elem(id, op.ch, origin, deleted));
        anyChange = true;
      }
    }
    if (anyChange) {
      this.#invalidateOrdered();
    }
    return this.text() !== before;
  }
}
