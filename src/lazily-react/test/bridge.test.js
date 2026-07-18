// Bridge contract tests — framework-agnostic. These pin the lazily →
// useSyncExternalStore contract without React, exercising the equality-guard
// distinction that separates useReactiveMemo/useSignal (guard) from useSlot
// (no guard). Each test mirrors a lazily reactive property.
//
// `createLazilySubscription` registers the dependency edge on its initial forced
// run but does NOT call onChange then (store contract: subscribe notifies only
// future changes). So call counts below are zero immediately after subscribing.

import assert from "node:assert/strict";
import test from "node:test";

import { Context } from "@lazily-hub/lazily-js/reactive";
import { createLazilySubscription, readHandle } from "../src/bridge.js";

test("subscribe: onChange fires exactly once per dependent cell write", () => {
  const ctx = new Context();
  const a = ctx.cell(1);
  const calls = [];
  const unsub = createLazilySubscription(ctx, a, () => calls.push("change"));

  ctx.setCell(a, 2);
  assert.deepEqual(calls, ["change"]);
  assert.equal(readHandle(ctx, a), 2, "getSnapshot reflects the new value");

  unsub();
  ctx.setCell(a, 3);
  assert.deepEqual(calls, ["change"], "unsubscribe stops further notifications");
  assert.equal(readHandle(ctx, a), 3);
});

test("subscribe: equal cell write does not fire (cell deep-PartialEq guard)", () => {
  const ctx = new Context();
  const a = ctx.cell(1);
  let calls = 0;
  createLazilySubscription(ctx, a, () => calls++);

  ctx.setCell(a, 1); // equal — no-op at the cell
  assert.equal(calls, 0);
});

test("subscribe: structurally-equal OBJECT write is a no-op (deep equality)", () => {
  const ctx = new Context();
  const a = ctx.cell({ x: 1 });
  const before = readHandle(ctx, a);
  let calls = 0;
  createLazilySubscription(ctx, a, () => calls++);

  ctx.setCell(a, { x: 1 }); // new ref but structurally equal → cell guard no-ops
  assert.equal(calls, 0);
  assert.equal(readHandle(ctx, a), before, "same reference preserved");
});

test("subscribe: memo suppresses onChange on structurally-equal recompute; slot propagates", () => {
  const ctx = new Context();
  const a = ctx.cell(1);
  // Fresh object each recompute. memo's deep-equality guard sees { n: 1 } === { n: 1 }
  // and suppresses; slot has no guard so it always propagates.
  const m = ctx.memo(() => ({ n: ctx.getCell(a) % 2 }));
  const s = ctx.slot(() => ({ n: ctx.getCell(a) % 2 }));

  readHandle(ctx, m); // materialize
  readHandle(ctx, s);

  let memoCalls = 0;
  let slotCalls = 0;
  createLazilySubscription(ctx, m, () => memoCalls++);
  createLazilySubscription(ctx, s, () => slotCalls++);

  ctx.setCell(a, 3); // 3 % 2 === 1, structurally equal to prior { n: 1 }
  assert.equal(memoCalls, 0, "memo must suppress onChange on equal recompute");
  assert.equal(slotCalls, 1, "slot has no guard, propagates on every invalidation");
});

test("subscribe: memo fires when recompute yields a structurally-different value", () => {
  const ctx = new Context();
  const a = ctx.cell(1);
  const m = ctx.memo(() => ({ n: ctx.getCell(a) % 2 }));
  readHandle(ctx, m);
  let calls = 0;
  createLazilySubscription(ctx, m, () => calls++);

  ctx.setCell(a, 2); // 2 % 2 === 0, different from prior { n: 1 }
  assert.equal(calls, 1);
  assert.equal(readHandle(ctx, m).n, 0);
});

test("subscribe: works for signal (eager derived)", () => {
  const ctx = new Context();
  const a = ctx.cell(10);
  const sig = ctx.signal(() => ctx.getCell(a) * 10);
  assert.equal(readHandle(ctx, sig), 100, "eager signal materializes immediately");

  let calls = 0;
  createLazilySubscription(ctx, sig, () => calls++);
  ctx.setCell(a, 3);
  assert.equal(calls, 1);
  assert.equal(readHandle(ctx, sig), 30);
});

test("subscribe: batched writes coalesce into one notification", () => {
  const ctx = new Context();
  const a = ctx.cell(1);
  const b = ctx.cell(10);
  const sum = ctx.memo(() => ctx.getCell(a) + ctx.getCell(b));
  readHandle(ctx, sum);
  let calls = 0;
  createLazilySubscription(ctx, sum, () => calls++);

  ctx.batch(() => {
    ctx.setCell(a, 2);
    ctx.setCell(b, 20);
  });
  assert.equal(calls, 1, "two batched writes → one coalesced notification");
  assert.equal(readHandle(ctx, sum), 22);
});

test("getSnapshot stability: equal write keeps the reference; real change yields a new one", () => {
  const ctx = new Context();
  const a = ctx.cell({ x: 1 });
  const snap1 = readHandle(ctx, a);
  const snap2 = readHandle(ctx, a);
  assert.equal(snap1, snap2, "identical reference while unchanged");

  ctx.setCell(a, { x: 1 }); // structurally equal → deep guard no-op → same ref
  assert.equal(readHandle(ctx, a), snap1, "structural-equal write keeps the reference");

  ctx.setCell(a, { x: 2 }); // different → new reference
  assert.notEqual(readHandle(ctx, a), snap1, "real change yields a new reference");
});
