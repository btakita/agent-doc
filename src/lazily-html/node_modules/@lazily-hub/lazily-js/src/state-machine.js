// Flat finite-state-machine kernel (lazily-spec/docs/state-machine.md) — the
// native JavaScript counterpart of lazily-kt's `StateMachine` and the Lean
// `LazilyFormal.StateMachine`. It is the kernel a single-region state chart
// compiles down to (state-machine.md: "a flat StateMachine is the degenerate
// chart with no nesting").
//
// A state machine is **compute, not protocol**: it is never serialized as a
// distinct wire kind — only its current state crosses IPC/FFI as an ordinary
// cell `Payload`. The state lives in a reactive `Cell` so any slot, signal, or
// effect that reads `state` is automatically invalidated when the machine
// transitions. The transition function is pure `(state, event) -> next | null`:
// a non-null result accepts the event; `null` rejects it (guard). A
// self-transition that returns an equal state is accepted but suppressed by the
// cell's `==` guard, so no downstream cascade fires.

import { SignalHandle } from "./reactive.js";

export class StateMachine {
  #ctx;
  #cell;
  #transition;

  constructor(ctx, initial, transition) {
    if (typeof transition !== "function") {
      throw new TypeError("StateMachine transition must be a function");
    }
    this.#ctx = ctx;
    this.#transition = transition;
    this.#cell = ctx.source(initial);
  }

  // Send an event. Returns `true` if accepted (non-null), `false` if rejected
  // (`null`/guard). An accepted transition to an equal state returns `true`
  // but the `==` guard suppresses invalidation (no downstream cascade).
  send(event) {
    const current = this.#ctx.get(this.#cell);
    const next = this.#transition(current, event);
    if (next === null || next === undefined) {
      return false;
    }
    this.#ctx.set(this.#cell, next);
    return true;
  }

  get state() {
    return this.#ctx.get(this.#cell);
  }

  // The underlying active-state cell handle, for reactive composition.
  stateHandle() {
    return this.#cell;
  }

  // Register an effect firing with `(old, new)` on each *real* state change.
  // Not called on registration; only on subsequent changes. Returns a handle
  // whose disposal stops further callbacks.
  onTransition(handler) {
    let prev = this.state;
    // Reads the state cell through the value-threaded compute view `c`
    // (#lzcellkernel) so the effect subscribes to state changes — the `this.state`
    // getter reads through a captured `ctx` (untracked) and would not subscribe.
    return this.#ctx.effect((c) => {
      const current = c.get(this.#cell);
      if (prev !== current) {
        handler(prev, current);
      }
      prev = current;
      return null;
    });
  }

  // An eager signal that is `true` while in `target`, else `false`.
  stateIs(target) {
    // The computed reads the state cell through its view `c`; the puller effect
    // reads the slot through its view (#lzcellkernel — sole tracking surface).
    const slot = this.#ctx.computed((c) => c.get(this.#cell) === target);
    const effect = this.#ctx.effect((c) => {
      c.get(slot);
      return null;
    });
    return new SignalHandle(slot, effect);
  }
}
