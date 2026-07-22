// Preact-named hooks, Option-A semantics.
//
// THE SEMANTIC DEVIATION (documented in README): `useState` / `useReducer`
// return a Solid-style **accessor** as the first element, not a snapshot value.
// `const [count, setCount] = useState(0)` — `count` is a `() => T` getter, not
// the bare `T`. This is the only way to make JSX `{count}` reactive without a
// compiler: the renderer detects a function-valued expression and binds it in
// an effect. Reading the accessor inside any tracked effect registers the lazily
// dependency edge, so `setCount(v)` patches exactly the DOM expressions that
// read it — never the component body.
//
// `useSignal` is provided as an alias for Solid familiarity.

import {
  useHook,
  withInstance,
  getCurrentInstance,
  depsChanged,
  getRenderMode,
  source,
  effect,
  onCleanup,
  getOwner,
} from "./instance.js";
import { memo } from "./owner.js";
import { readContextAccessor } from "./context.js";

// -- useState / useSignal --------------------------------------------------

export function useState(initializer) {
  return useHook(() => {
    const s = source(
      typeof initializer === "function" ? initializer() : initializer,
    );
    return [s.get, s.set];
  });
}

export const useSignal = useState;

// -- useReducer ------------------------------------------------------------

export function useReducer(reducer, initialArg, init) {
  return useHook(() => {
    const initial = init ? init(initialArg) : initialArg;
    const s = source(initial);
    const dispatch = (action) => s.set(reducer(s.get(), action));
    return [s.get, dispatch];
  });
}

// -- useEffect / useLayoutEffect -------------------------------------------
//
// lazily effects are synchronous (flush on cell write), so useEffect and
// useLayoutEffect share one implementation — both run before paint. The deps
// array guards re-subscription: when deps change we dispose the prior lazily
// effect and create a new one, running the user callback fresh.

function useEffectImpl(setup, deps) {
  // #lzdomservernoop: on the server, effect hooks are no-ops. The component
  // body still runs once (so useState/useMemo produce their initial values),
  // but no DOM side-effects are scheduled.
  if (getRenderMode() === "server") return;
  const inst = getCurrentInstance();
  useHook(() => ({})); // slot holds { handle, prevDeps, prevCleanup }
  const idx = inst._hookIndex - 1;
  const slot = inst._hooks[idx];
  const firstRun = !slot.initialized;
  if (firstRun || depsChanged(slot.prevDeps, deps)) {
    if (slot.handle) {
      try {
        getOwner().ctx.disposeEffect(slot.handle);
      } catch (_) {}
      slot.handle = null;
    }
    slot.prevCleanup =
      typeof slot.prevCleanup === "function" ? slot.prevCleanup : null;
    const owner = getOwner();
    slot.handle = owner.ctx.effect(() => {
      if (slot.prevCleanup) {
        try {
          slot.prevCleanup();
        } catch (_) {}
        slot.prevCleanup = null;
      }
      const ret = setup();
      slot.prevCleanup = typeof ret === "function" ? ret : null;
      return null;
    });
    owner.effects.push(slot.handle);
    slot.prevDeps = deps;
    slot.initialized = true;
    onCleanup(() => {
      if (slot.prevCleanup) {
        try {
          slot.prevCleanup();
        } catch (_) {}
      }
      if (slot.handle) {
        try {
          getOwner().ctx.disposeEffect(slot.handle);
        } catch (_) {}
      }
    });
  }
}

export function useEffect(setup, deps) {
  useEffectImpl(setup, deps);
}

export function useLayoutEffect(setup, deps) {
  useEffectImpl(setup, deps);
}

// -- useRef ----------------------------------------------------------------

export function useRef(initial) {
  return useHook(() => ({ current: initial }));
}

// -- useMemo / useCallback -------------------------------------------------
//
// #lzdomautotrack: in Option A, reactivity is automatic. useMemo's factory
// reads accessors that register lazily edges, so the returned memo recomputes
// whenever those inputs change — the `deps` array is a coarse recreate-hint,
// not the reactive driver. useCallback is a stable identity helper.

export function useMemo(factory, deps) {
  const slot = useHook(() => ({ memo: null, deps: undefined }));
  if (slot.memo === null || depsChanged(slot.deps, deps)) {
    slot.memo = memo(factory);
    slot.deps = deps;
  }
  return slot.memo;
}

export function useCallback(fn, deps) {
  const slot = useHook(() => ({ fn: null, deps: undefined }));
  if (slot.fn === null || depsChanged(slot.deps, deps)) {
    slot.fn = fn;
    slot.deps = deps;
  }
  return slot.fn;
}

// -- useContext ------------------------------------------------------------

export function useContext(context) {
  const inst = getCurrentInstance();
  useHook(() => ({}));
  const idx = inst._hookIndex - 1;
  const slot = inst._hooks[idx];
  if (!slot.accessor) {
    slot.accessor = readContextAccessor(context);
  }
  // Return the accessor so JSX `{theme()}` is reactive. For Preact-shape
  // callers that expect the bare value, they may call it themselves.
  return slot.accessor;
}

// -- useId / useDebugValue (stubs for compat) ------------------------------

let nextUseId = 0;
export function useId() {
  return useHook(() => `:r${nextUseId++}:`);
}

export function useDebugValue(_value, _formatter) {
  /* no-op in non-dev */
}

export { withInstance, getCurrentInstance };
