import type { Context, CellHandle, EffectHandle, SignalHandle } from "./reactive.js";

export type TransitionFn<S, E> = (state: S, event: E) => S | null;

export type TransitionListener<S> = (old: S, next: S) => void;

export class StateMachine<S, E> {
  constructor(ctx: Context, initial: S, transition: TransitionFn<S, E>);
  get state(): S;
  send(event: E): boolean;
  stateHandle(): CellHandle<S>;
  onTransition(handler: TransitionListener<S>): EffectHandle;
  stateIs(target: S): SignalHandle<boolean>;
}
