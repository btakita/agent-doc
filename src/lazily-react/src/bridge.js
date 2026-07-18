// Framework-agnostic adapter between a lazily reactive graph and the
// `useSyncExternalStore` contract shared by React 18+ and Preact's `preact/compat`.
//
// lazily (see @lazily-hub/lazily-js `reactive.js`) exposes three handle kinds:
//   - CellHandle   (mutable source)
//   - SlotHandle   (lazy derived: `slot`/`computed` = no guard, `memo` = equality guard)
//   - SignalHandle (eager derived: memo slot + puller effect)
//
// `useSyncExternalStore(subscribe, getSnapshot)` needs two things:
//   - subscribe(onChange): register interest, return an unsubscribe.
//   - getSnapshot(): read the current value. Must be referentially stable across
//     calls while the value is unchanged (else React loops "getSnapshot should be
//     cached"). lazily caches node values and only changes reference on a real
//     change, so this holds by construction for every primitive.
//
// The bridge registers a lazily `effect` whose body reads the handle (which
// auto-registers the dependency edge) and then calls `onChange`. lazily flushes
// effects synchronously before `setCell`/`batch` returns, matching the
// notify-then-read contract `useSyncExternalStore` expects.

import { CellHandle, SignalHandle } from "@lazily-hub/lazily-js/reactive";

/**
 * Read a lazily handle of any kind from `ctx`, dispatching on handle class.
 *
 * - `SignalHandle` → `ctx.getSignal` (reads the backing memo slot)
 * - `CellHandle`   → `ctx.getCell`
 * - `SlotHandle`   → `ctx.get` (covers `slot`/`computed`/`memo`)
 *
 * Inside a tracked computation (an `effect`/`slot` body) the read also registers
 * the dependency edge; outside one it is a plain read.
 *
 * @template T
 * @param {import("@lazily-hub/lazily-js/reactive").Context} ctx
 * @param {import("@lazily-hub/lazily-js/reactive").CellHandle<T> | import("@lazily-hub/lazily-js/reactive").SlotHandle<T> | import("@lazily-hub/lazily-js/reactive").SignalHandle<T>} handle
 * @returns {T}
 */
export function readHandle(ctx, handle) {
  if (handle instanceof SignalHandle) return ctx.getSignal(handle);
  if (handle instanceof CellHandle) return ctx.getCell(handle);
  return ctx.get(handle);
}

/**
 * Subscribe a React/Preact `onChange` callback to a lazily handle.
 *
 * Creates a lazily effect that (a) reads the handle — registering the dependency
 * edge — and (b) calls `onChange` whenever the handle's value invalidates. lazily
 * effects flush synchronously on write, so `onChange` fires before the mutating
 * `setCell`/`batch` returns.
 *
 * Equality-guard semantics come from the handle itself:
 *   - a `memo`/`signal` whose recompute yields an equal value suppresses the
 *     effect run entirely (see `recomputeSlotNow` → `effectShouldRun` in lazily's
 *     `reactive.js`), so `onChange` is NOT called and React does not re-render;
 *   - a no-guard `slot`/`computed` always propagates, so `onChange` fires on every
 *     upstream invalidation regardless of whether the value actually changed.
 *
 * @param {import("@lazily-hub/lazily-js/reactive").Context} ctx
 * @param {import("@lazily-hub/lazily-js/reactive").CellHandle | import("@lazily-hub/lazily-js/reactive").SlotHandle | import("@lazily-hub/lazily-js/reactive").SignalHandle} handle
 * @param {() => void} onChange React's re-render trigger from `useSyncExternalStore`.
 * @returns {() => void} unsubscribe — disposes the underlying effect (tears down
 *   the dependency edge and removes it from the effect queue).
 */
export function createLazilySubscription(ctx, handle, onChange) {
  // lazily effects force-run once on creation (#lzjseffort) — that first run is
  // what registers the dependency edge. The store contract (subscribe) should not
  // fire `onChange` during subscription itself, so the first invocation registers
  // the edge and materializes the value but skips the notification; React reads
  // the initial value via getSnapshot separately.
  let initialized = false;
  const eff = ctx.effect(() => {
    readHandle(ctx, handle); // always: register dependency edge + read current value
    if (initialized) onChange();
    initialized = true;
    return null;
  });
  return () => ctx.disposeEffect(eff);
}
