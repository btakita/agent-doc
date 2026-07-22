// Hook dispatch — renderer-agnostic.
//
// React/Preact hooks dispatch by index against a per-component "instance" that
// holds the hook slots. We do the same, but each slot owns lazily reactive
// primitives (a `source` cell, an `effect` handle, etc.) so a `setState` write
// flows through lazily's graph into the per-expression DOM bindings — never a
// component-body re-execution (Option A, #lzdomnoTrackBody).
//
// Both function components and class components route through `withInstance`.

import { source, effect, onCleanup, getOwner } from "./owner.js";

// RENDER_MODE: "browser" (default) binds real effects; "server" no-ops effect
// hooks so SSR doesn't schedule DOM side-effects. Set via setRenderMode from
// server.js during renderToString.
let RENDER_MODE = "browser";

export function getRenderMode() {
  return RENDER_MODE;
}

export function setRenderMode(mode) {
  RENDER_MODE = mode;
}

let CURRENT_INSTANCE = null;

export function getCurrentInstance() {
  return CURRENT_INSTANCE;
}

export function withInstance(instance, fn) {
  const prev = CURRENT_INSTANCE;
  CURRENT_INSTANCE = instance;
  instance._hookIndex = 0;
  try {
    return fn();
  } finally {
    CURRENT_INSTANCE = prev;
  }
}

// Allocate (first render) or retrieve (subsequent render of the same instance)
// the slot at the current hook index. `init` runs only once per slot.
export function useHook(init) {
  const inst = CURRENT_INSTANCE;
  if (!inst) {
    throw new Error("lazily-html: hook called outside a component render");
  }
  const idx = inst._hookIndex++;
  let slot = inst._hooks[idx];
  if (slot === undefined) {
    slot = init();
    inst._hooks[idx] = slot;
  }
  return slot;
}

// -- React-style hook deps guard -------------------------------------------

export function depsChanged(prev, next) {
  if (!prev || !next) return true;
  if (prev.length !== next.length) return true;
  for (let i = 0; i < prev.length; i++) {
    if (!Object.is(prev[i], next[i])) return true;
  }
  return false;
}

// Re-export the reactive primitives hooks build on, so the hooks module is the
// single import site for lazily bridge bits.
export { source, effect, onCleanup, getOwner };
