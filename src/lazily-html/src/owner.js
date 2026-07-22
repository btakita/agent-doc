// lazily ↔ renderer bridge.
//
// Adapts a lazily reactive `Context` into the small reactive surface the DOM
// renderer needs: a disposal-scoped `root`, `effect`/`memo`/`onCleanup`,
// `source` (writable Source as accessor), `mergeProps`, and an `untrack` shim.
//
// This is the `rxcore` equivalent: dom-expressions imports
// `{ root, effect, memo, getOwner, createComponent, sharedConfig, untrack,
//   mergeProps }` from its reactive core; lazily-html implements the same
// surface on `@lazily-hub/lazily-js/reactive`.
//
// #lzdomnoTrackBody: a component body (function or class `render()`) is NEVER
// invoked inside a lazily tracking compute. Any direct `ctx.get` read in
// render is therefore untracked — only the per-expression `effect()` wrappers
// created during DOM materialization subscribe. This is what makes setState a
// Source write that patches a single DOM node, not a full re-render (the
// Solid/relementjs model).

import { createContext } from "@lazily-hub/lazily-js/reactive";

// One lazily Context per render tree. Defaults to a module-global; callers can
// override per render via `setReactiveContext` for isolation (e.g. tests).
let DEFAULT_CTX = null;
let CURRENT_COMPUTE = null;

export function getReactiveContext() {
  if (!DEFAULT_CTX) DEFAULT_CTX = createContext();
  return DEFAULT_CTX;
}

export function setReactiveContext(ctx) {
  DEFAULT_CTX = ctx;
}

export function resetReactiveContext() {
  DEFAULT_CTX = null;
}

function runWithCompute(compute, fn) {
  const prev = CURRENT_COMPUTE;
  CURRENT_COMPUTE = compute;
  try {
    return fn();
  } finally {
    CURRENT_COMPUTE = prev;
  }
}

export function read(handle, ctx = getReactiveContext()) {
  return (CURRENT_COMPUTE || ctx).get(handle);
}

// -- Owner stack -----------------------------------------------------------

let CURRENT_OWNER = null;

export function getOwner() {
  return CURRENT_OWNER;
}

export function runWithOwner(owner, fn) {
  const prev = CURRENT_OWNER;
  CURRENT_OWNER = owner;
  try {
    return fn();
  } finally {
    CURRENT_OWNER = prev;
  }
}

// -- root: disposal scope --------------------------------------------------

export function root(fn, ownerOverride) {
  const ctx = (ownerOverride && ownerOverride.ctx) || getReactiveContext();
  const newOwner = {
    ctx,
    parent: CURRENT_OWNER,
    effects: [],
    cleanups: [],
    hooks: null,
    hookIndex: 0,
    component: null,
  };
  const prev = CURRENT_OWNER;
  CURRENT_OWNER = newOwner;
  let disposed = false;
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    const cleanups = newOwner.cleanups;
    for (let i = cleanups.length - 1; i >= 0; i--) {
      try {
        cleanups[i]();
      } catch (_) {
        /* swallow cleanup errors */
      }
    }
    cleanups.length = 0;
    for (const handle of newOwner.effects) {
      try {
      handle.dispose();
      } catch (_) {
        /* already disposed */
      }
    }
    newOwner.effects.length = 0;
  };
  try {
    return fn(dispose);
  } finally {
    CURRENT_OWNER = prev;
  }
}

// -- effect: reactive side-effect with prev-threading ----------------------

// dom-expressions threads the previous return value across re-runs
// (`effect(current => insertExpression(parent, accessor(), current, marker))`).
// lazily's EffectRun is `() => cleanup|null`; we adapt: the wrapped fn receives
// prev and its return becomes next-run's prev (Solid semantics), and lazily's
// cleanup-return is unused (we use onCleanup for explicit cleanup).
export function effect(fn, def) {
  const owner = CURRENT_OWNER;
  if (!owner) {
    // No owner: run in an ephemeral root so the effect is still tracked.
    return root(() => effect(fn, def));
  }
  const { ctx, effects } = owner;
  let prev = def;
  const handle = ctx.effect((compute) => {
    prev = runWithCompute(compute, () => fn(prev, prev));
    return null;
  });
  effects.push(handle);
  return handle;
}

// -- memo: == guarded derived accessor -------------------------------------

export function memo(fn) {
  const ctx = (CURRENT_OWNER && CURRENT_OWNER.ctx) || getReactiveContext();
  const handle = ctx.computed((compute) => runWithCompute(compute, fn));
  const accessor = () => read(handle, ctx);
  accessor.handle = handle;
  return accessor;
}

// -- source: writable Source as { get, set } -------------------------------

export function source(initial) {
  const ctx = (CURRENT_OWNER && CURRENT_OWNER.ctx) || getReactiveContext();
  const handle = ctx.source(initial);
  return {
    get: () => read(handle, ctx),
    set: (v) => ctx.set(handle, v),
    handle,
  };
}

// -- onCleanup -------------------------------------------------------------

export function onCleanup(fn) {
  const owner = CURRENT_OWNER;
  if (!owner) return;
  owner.cleanups.push(fn);
}

// -- untrack (v0.1 shim) ---------------------------------------------------
//
// Real untrack requires lazily to expose its tracking-stack toggle (a one-line
// addition upstream). The passthrough is safe for lazily-html's current code
// paths: component bodies are already untracked (#lzdomnoTrackBody), and ref
// callbacks fire outside any compute. The single place true untrack would
// matter — calling a function component inside a tracking compute — is avoided
// by construction (see create-element.js).

export function untrack(fn) {
  return fn();
}

// -- sharedConfig: hydration seam (stub for v0.1) --------------------------

export const sharedConfig = {
  context: null,
  registry: null,
  events: null,
  completed: null,
  load: null,
  has: null,
  gather: null,
  done: false,
  getNextContextId: null,
};

// -- mergeProps: Solid-style proxy merge preserving accessors --------------

export function mergeProps(...sources) {
  const out = {};
  for (const src of sources) {
    if (!src) continue;
    for (const key of Object.keys(src)) {
      const v = src[key];
      if (typeof v === "function" && v._isAccessor) {
        Object.defineProperty(out, key, {
          get: v,
          enumerable: true,
          configurable: true,
        });
      } else {
        out[key] = v;
      }
    }
  }
  return out;
}

// -- createComponent: rxcore contract -------------------------------------
//
// Solid's rxcore expects `createComponent(Comp, props)` to call the component
// function untracked (so the component body's reads bind individually, not
// against a wrapping compute — #lzdomnoTrackBody). dom-expressions' client.js
// re-exports this; the full renderer's hyperscript path invokes it for function
// components. Class components and hook-instance setup are handled by the
// renderer's mount layer (mount.js), not here.

export function createComponent(Comp, props) {
  return untrack(() => Comp(props));
}

// Mark an accessor function so mergeProps preserves it as a getter.
export function markAccessor(fn) {
  fn._isAccessor = true;
  return fn;
}
