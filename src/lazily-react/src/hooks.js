// React / Preact hooks over a lazily reactive `Context`.
//
// Imports from `react`. Preact users alias `react` → `preact/compat` (the
// standard ecosystem convention); `useSyncExternalStore` is exported by both
// `react` (>=18) and `preact/compat` (>=10.16). We destructure from the default
// import to avoid CJS named-export interop differences across bundlers/runtimes.
//
// Eager derived values are intentionally NOT exposed here: `Signal` is retired as
// a lazily primitive (`Signal ≡ Slot.eager`), and a React binding gains nothing
// from eagerness — React only renders on invalidation, and `getSnapshot` reads the
// (lazily-recomputed-on-read) slot, so it always sees the fresh value with no
// stale-frame risk. The meaningful axis for derived hooks is the equality guard:
// `useReactiveMemo` (memo, suppresses equal recomputes) vs `useSlot` (no guard).

import React from "react";
import { createLazilySubscription, readHandle } from "./bridge.js";

const { createContext, useContext, useCallback, useEffect, useRef, useSyncExternalStore } = React;

// -- Provider -----------------------------------------------------------------

const LazilyContext = createContext(null);

/**
 * Provide a lazily reactive `Context` to the tree. A `Context` owns the whole
 * reactive graph (Cell/Slot/Effect nodes); one at the app root is the common
 * pattern. Per-feature `Context`s are fine too.
 *
 * @param {{ context: import("@lazily-hub/lazily-js/reactive").Context, children: import("react").ReactNode }} props
 */
export function LazilyProvider({ context, children }) {
  return React.createElement(LazilyContext.Provider, { value: context }, children);
}

/**
 * Read the lazily `Context` from the nearest `LazilyProvider`. Throws if missing.
 * @returns {import("@lazily-hub/lazily-js/reactive").Context}
 */
export function useLazilyContext() {
  const ctx = useContext(LazilyContext);
  if (ctx === null || ctx === undefined) {
    throw new Error(
      "lazily-react: useLazily hooks require a <LazilyProvider context={ctx}>.",
    );
  }
  return ctx;
}

// -- deps helper --------------------------------------------------------------

function shallowEqualDeps(a, b) {
  if (a === b) return true;
  if (!a || !b || a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (!Object.is(a[i], b[i])) return false;
  }
  return true;
}

// -- strict-mode-safe dispose -------------------------------------------------

/**
 * Dispose a lazily handle on REAL unmount or deps-change, without firing during
 * React 18 strict-mode's simulated mount (setup → cleanup → setup with the SAME
 * handle). Disposal is deferred one microtask; if the same handle is re-subscribed
 * before the microtask runs (strict-mode remount), it is kept. A deps-change (the
 * handle moved on to a new value) or a real unmount disposes the stale handle.
 *
 * `disposeFn(ctx, h)` closes over the stable `ctx`, so it is not listed in the
 * effect deps.
 *
 * @param {import("@lazily-hub/lazily-js/reactive").Context} ctx
 * @param {any} handle
 * @param {(ctx: import("@lazily-hub/lazily-js/reactive").Context, handle: any) => void} disposeFn
 */
function useStableDispose(ctx, handle, disposeFn) {
  const mountedRef = useRef(false);
  const latestRef = useRef(handle);
  latestRef.current = handle;
  useEffect(() => {
    mountedRef.current = true;
    const h = handle;
    return () => {
      mountedRef.current = false;
      queueMicrotask(() => {
        if (mountedRef.current && latestRef.current === h) return;
        disposeFn(ctx, h);
      });
    };
  }, [ctx, handle]);
}

// -- subscribe / snapshot (stable per (ctx, handle) pair) ----------------------

function useLazilySubscription(ctx, handle) {
  const subscribe = useCallback(
    (onChange) => createLazilySubscription(ctx, handle, onChange),
    [ctx, handle],
  );
  const getSnapshot = useCallback(() => readHandle(ctx, handle), [ctx, handle]);
  return useSyncExternalStore(subscribe, getSnapshot);
}

// -- Public hooks -------------------------------------------------------------

/**
 * Subscribe (read-only) to an externally-created lazily handle of any kind.
 * Use this to read a handle owned outside React (e.g. a module-scoped cell). The
 * handle's lifetime is the caller's responsibility — `useLazily` does NOT dispose
 * it on unmount; it only unsubscribes. Writes happen via `ctx.setCell` directly.
 *
 * @template T
 * @param {import("./bridge.js").LazilyHandle<T>} handle
 * @returns {T}
 */
export function useLazily(handle) {
  const ctx = useLazilyContext();
  return useLazilySubscription(ctx, handle);
}

/**
 * Component-local mutable source. Like `useState`, but the value lives in a
 * lazily cell bound to the owning `Context`. Returns `[value, setValue]`.
 *
 * `setValue` accepts a value or an updater `(prev) => next`. The cell is disposed
 * on real unmount (strict-mode-safe) via `ctx.disposeCell`.
 *
 * @template T
 * @param {T | (() => T)} initial
 * @returns {[T, (next: T | ((prev: T) => T)) => void]}
 */
export function useCell(initial) {
  const ctx = useLazilyContext();
  const ref = useRef(null);
  if (ref.current === null) {
    ref.current =
      typeof initial === "function" ? ctx.cell(initial()) : ctx.cell(initial);
  }
  const handle = ref.current;
  useStableDispose(ctx, handle, (c, h) => c.disposeCell(h));
  const value = useLazilySubscription(ctx, handle);
  const setValue = useCallback(
    (next) => {
      const resolved =
        typeof next === "function" ? next(readHandle(ctx, handle)) : next;
      ctx.setCell(handle, resolved);
    },
    [ctx, handle],
  );
  return [value, setValue];
}

// -- Lazy derived hooks -------------------------------------------------------

const EMPTY = Object.freeze([]);

/**
 * @template T
 * @param {import("@lazily-hub/lazily-js/reactive").Context} ctx
 * @param {() => T} compute
 * @returns {import("@lazily-hub/lazily-js/reactive").SlotHandle<T>}
 * @internal */
function makeSlot(ctx, compute) {
  return ctx.slot(compute);
}

/**
 * @template T
 * @param {import("@lazily-hub/lazily-js/reactive").Context} ctx
 * @param {() => T} compute
 * @returns {import("@lazily-hub/lazily-js/reactive").SlotHandle<T>}
 * @internal */
function makeMemo(ctx, compute) {
  return ctx.memo(compute);
}

/**
 * Core for the lazy derived hooks. Creates a lazily slot via `factory` and
 * re-subscribes React to it. When `deps` change, a new slot is created and the
 * stale one is disposed (strict-mode-safe). The latest `compute` closure is always
 * used via a ref, so a re-render with unchanged deps still sees fresh captured
 * values on the next invalidation.
 *
 * @template T
 * @param {(ctx: import("@lazily-hub/lazily-js/reactive").Context, compute: () => T) => import("@lazily-hub/lazily-js/reactive").SlotHandle<T>} factory
 * @param {() => T} compute
 * @param {unknown[] | undefined} deps
 * @returns {T}
 * @internal */
function useDerived(factory, compute, deps) {
  const ctx = useLazilyContext();
  const handleRef = useRef(null);
  const computeRef = useRef(compute);
  computeRef.current = compute; // always latest closure; slot recomputes only on invalidation
  const depsArr = deps === undefined ? EMPTY : deps;
  const depsRef = useRef(depsArr);
  if (handleRef.current === null || !shallowEqualDeps(depsRef.current, depsArr)) {
    handleRef.current = factory(ctx, () => computeRef.current());
    depsRef.current = depsArr;
  }
  const handle = handleRef.current;
  useStableDispose(ctx, handle, (c, h) => c.disposeSlot(h));
  return useLazilySubscription(ctx, handle);
}

/**
 * Lazy derived value, **no** equality guard. Re-renders on every upstream
 * invalidation even when the recomputed value is structurally equal. Escape
 * hatch for when `defaultEqual` is more expensive than the render, or when every
 * invalidation must propagate.
 *
 * @template T
 * @param {() => T} compute
 * @param {unknown[]} [deps] dep list; the slot is recreated when these change.
 * @returns {T}
 */
export function useSlot(compute, deps) {
  return useDerived(makeSlot, compute, deps);
}

/**
 * Lazy derived value, **with** an equality guard (`ctx.memo`). Equal recomputes
 * suppress the re-render. This is the default choice for derived state; prefer it
 * over `useSlot` unless the equality check itself is the bottleneck.
 *
 * @template T
 * @param {() => T} compute
 * @param {unknown[]} [deps]
 * @returns {T}
 */
export function useReactiveMemo(compute, deps) {
  return useDerived(makeMemo, compute, deps);
}
