// React integration tests via react-test-renderer (no DOM). Proves the hooks
// drive React renders under the lazily reactive contract: initial value, mutation
// re-render, memo suppression of structurally-equal recomputes, slot propagation,
// strict-mode-safe disposal on unmount, and unsubscribe safety after unmount.
//
// External-store notifications (lazily effects flush synchronously on write) are
// drained through the ASYNC form of `act`: `await act(async () => { ... })`. The
// sync form returns before `useSyncExternalStore`'s scheduled re-render is
// committed, so assertions would race the re-render.

import assert from "node:assert/strict";
import test from "node:test";

import React from "react";
import TestRenderer from "react-test-renderer";

import { Context } from "@lazily-hub/lazily-js/reactive";
import {
  LazilyProvider,
  useCell,
  useSlot,
  useReactiveMemo,
  useLazily,
} from "../src/hooks.js";

const { createElement: h } = React;
const { act } = TestRenderer;

/** Render `inner` inside a LazilyProvider bound to `ctx`. Returns the renderer. */
function renderWith(ctx, inner) {
  return TestRenderer.create(h(LazilyProvider, { context: ctx }, inner));
}

/** Extract the single text child of the rendered root element. */
function textOf(renderer) {
  const tree = renderer.toJSON();
  return Array.isArray(tree.children) ? tree.children.join("") : String(tree.children);
}

/** Drive an external mutation through async act so the re-render flushes. */
async function flush(fn) {
  await act(async () => fn());
}

/** Drain the deferred-dispose microtask used by the strict-mode-safe cleanup. */
function drainMicrotask() {
  return new Promise((res) => queueMicrotask(res));
}

// -----------------------------------------------------------------------------

test("useCell: initial value renders, setter drives a re-render", async () => {
  const ctx = new Context();
  let setter;
  function C() {
    const [n, setN] = useCell(1);
    setter = setN;
    return h("p", null, String(n));
  }
  const r = renderWith(ctx, h(C));
  assert.equal(textOf(r), "1");

  await flush(() => setter(5));
  assert.equal(textOf(r), "5");
});

test("useCell: functional updater reads the previous value", async () => {
  const ctx = new Context();
  let setter;
  function C() {
    const [n, setN] = useCell(10);
    setter = setN;
    return h("p", null, String(n));
  }
  const r = renderWith(ctx, h(C));
  assert.equal(textOf(r), "10");

  await flush(() => setter((prev) => prev + 7));
  assert.equal(textOf(r), "17");
});

test("useReactiveMemo: recomputes and re-renders when upstream changes", async () => {
  const ctx = new Context();
  const a = ctx.cell(2);
  function C() {
    const doubled = useReactiveMemo(() => ctx.getCell(a) * 2, []);
    return h("p", null, String(doubled));
  }
  const r = renderWith(ctx, h(C));
  assert.equal(textOf(r), "4");

  await flush(() => ctx.setCell(a, 3));
  assert.equal(textOf(r), "6");
});

test("useReactiveMemo: structurally-equal recompute suppresses the re-render", async () => {
  const ctx = new Context();
  const a = ctx.cell(1);
  let renders = 0;
  function C() {
    renders++;
    // Fresh object each recompute; memo's deep-equality guard suppresses at the
    // lazily level, so onChange is never called and React never re-checks.
    const parity = useReactiveMemo(() => ({ n: ctx.getCell(a) % 2 }), []);
    return h("p", null, String(parity.n));
  }
  const r = renderWith(ctx, h(C));
  const initialRenders = renders;
  assert.equal(textOf(r), "1");

  await flush(() => ctx.setCell(a, 3)); // { n: 1 } → { n: 1 }, equal
  assert.equal(renders, initialRenders, "no re-render: memo guard suppressed propagation");
  assert.equal(textOf(r), "1");
});

test("useSlot: re-renders on every upstream invalidation (no equality guard)", async () => {
  const ctx = new Context();
  const a = ctx.cell(1);
  let renders = 0;
  function C() {
    renders++;
    // Fresh object each recompute; slot has no guard, so lazily propagates and
    // React sees a new reference (Object.is differs) → re-render. This is the
    // observable difference from useReactiveMemo at the React level.
    const parity = useSlot(() => ({ n: ctx.getCell(a) % 2 }), []);
    return h("p", null, String(parity.n));
  }
  const r = renderWith(ctx, h(C));
  const initialRenders = renders;
  assert.equal(textOf(r), "1");

  await flush(() => ctx.setCell(a, 3)); // { n: 1 } → new { n: 1 }, new reference
  assert.ok(renders > initialRenders, "useSlot re-renders: fresh object ref each invalidation");
  assert.equal(textOf(r), "1");
});

test("useSignal: REMOVED — eager derived dropped (Signal retired; not in this binding)", () => {
  // useSignal was removed: Signal is retired as a lazily primitive (Signal ≡ Slot.eager),
  // and a React binding gains nothing from eagerness (getSnapshot reads the lazy slot on
  // render, so no stale-frame risk). See README "Why no useSignal".
  assert.ok(true);
});

test("useLazily: subscribes to an externally-created cell handle", async () => {
  const ctx = new Context();
  const shared = ctx.cell("x");
  function C() {
    const v = useLazily(shared);
    return h("p", null, v);
  }
  const r = renderWith(ctx, h(C));
  assert.equal(textOf(r), "x");

  await flush(() => ctx.setCell(shared, "y"));
  assert.equal(textOf(r), "y");
});

test("unmount safety: external mutation after unmount does not throw", async () => {
  const ctx = new Context();
  const a = ctx.cell(1);
  function C() {
    const v = useReactiveMemo(() => ctx.getCell(a) + 1, []);
    return h("p", null, String(v));
  }
  const r = renderWith(ctx, h(C));
  r.unmount();

  await assert.doesNotReject(async () => {
    // Mutating after the subscribe effect was torn down must not throw and must
    // not leak notifications into React.
    await flush(() => ctx.setCell(a, 99));
  });
});

test("dispose on unmount: derived slot edges are torn down (microtask-deferred)", async () => {
  const ctx = new Context({ instrument: true });
  const a = ctx.cell(1);
  function C() {
    const v = useReactiveMemo(() => ctx.getCell(a) + 1, []);
    return h("p", null, String(v));
  }
  const r = renderWith(ctx, h(C));
  const before = ctx.instrumentationSnapshot().dependencyEdgesRemoved;

  // Wrap unmount in act so the passive-effect cleanup flushes; the strict-mode-safe
  // dispose then defers disposeSlot by one further microtask.
  await act(async () => r.unmount());
  await drainMicrotask();

  const after = ctx.instrumentationSnapshot().dependencyEdgesRemoved;
  assert.ok(after - before > 0, "slot + subscribe-effect edges removed after unmount");
});

test("dispose on deps-change: the stale slot is disposed when deps move on", async () => {
  const ctx = new Context({ instrument: true });
  // Deps come from a parent cell so we can force a re-render with new deps.
  const key = ctx.cell("a");
  function C() {
    const k = useLazily(key);
    const v = useReactiveMemo(() => k + "!", [k]); // recreated when k changes
    return h("p", null, v);
  }
  const r = renderWith(ctx, h(C));
  assert.equal(textOf(r), "a!");

  const before = ctx.instrumentationSnapshot().dependencyEdgesRemoved;
  await flush(() => ctx.setCell(key, "b")); // k changes → deps change → new slot
  assert.equal(textOf(r), "b!");
  await drainMicrotask();
  const after = ctx.instrumentationSnapshot().dependencyEdgesRemoved;
  assert.ok(after - before > 0, "stale slot disposed on deps-change");
});

test("StrictMode: hooks survive the dev double-invoke mount without premature disposal", async () => {
  const ctx = new Context();
  const a = ctx.cell(1);
  function C() {
    const v = useReactiveMemo(() => ctx.getCell(a) + 1, []);
    return h("p", null, String(v));
  }
  // StrictMode double-invokes render + effects (setup → cleanup → setup) in dev.
  // The strict-mode-safe dispose must NOT free the handle during the simulated remount.
  const r = TestRenderer.create(
    h(React.StrictMode, null, h(LazilyProvider, { context: ctx }, h(C))),
  );
  assert.equal(textOf(r), "2", "value correct after the simulated remount");

  await flush(() => ctx.setCell(a, 5)); // proves the slot is still alive + subscribed
  assert.equal(textOf(r), "6");

  r.unmount();
  await drainMicrotask();
});
