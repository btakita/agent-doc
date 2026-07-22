// SyncedView — the client-side reactive replica for hydration.
//
// Conceptually a thin lazily-html-side specialization of lazily's `GraphView`:
// it owns a lazily reactive `Context` whose Sources hold the anchor values, so
// that `hydrate`'s effect bindings read through `view.get(id)` (registering a
// lazily dependency edge) and `view.set(id, v)` / future `applyDelta(delta)`
// propagate straight into the bound DOM expressions.
//
// v0.1: `applySnapshot` accepts the lazily-html anchor fragment
// `{ version, anchors: { id: value } }`. The API is shaped so that a future
// upgrade to the full lazily `Snapshot`/`Delta` wire types (binary payloads,
// capability filtering via `filterReadable`, live `applyDelta` streaming) is
// additive — the SyncedView surface (`applySnapshot` / `applyDelta` / `get` /
// `set`) stays the same.

import { createContext } from "@lazily-hub/lazily-js/reactive";

export class SyncedView {
  constructor() {
    this.ctx = createContext();
    this._cells = Object.create(null); // anchor id -> Source
  }

  applySnapshot(snapshot) {
    const anchors = (snapshot && snapshot.anchors) || snapshot || {};
    for (const id in anchors) {
      const value = anchors[id];
      if (this._cells[id]) {
        this.ctx.set(this._cells[id], value);
      } else {
        this._cells[id] = this.ctx.source(value);
      }
    }
  }

  // Placeholder for live updates — v0.1 ships static-snapshot hydration.
  // Future: accept a lazily Delta wire object and apply ops to the matching
  // cells (cellSet/slotValue/...), mirroring `GraphView.applyDelta`.
  applyDelta(_delta) {
    throw new Error("lazily-html: SyncedView.applyDelta is not implemented in v0.1 (static-snapshot hydration only)");
  }

  get(id) {
    const h = this._cells[id];
    if (!h) return undefined;
    return read(h, this.ctx);
  }

  set(id, value) {
    if (!this._cells[id]) {
      this._cells[id] = this.ctx.source(value);
    } else {
      this.ctx.set(this._cells[id], value);
    }
  }

  has(id) {
    return id in this._cells;
  }

  // Bind this view's lazily Context as the renderer's active context so that
// effect() calls inside hydrate() register Sources on this view's graph.
  activate() {
    // Local import to avoid a cycle (owner.js is the bridge).
    setReactiveContext(this.ctx);
  }
}

import { read, setReactiveContext } from "./owner.js";
