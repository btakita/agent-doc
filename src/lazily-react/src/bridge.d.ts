import type {
  Context,
  CellHandle,
  SlotHandle,
  SignalHandle,
} from "@lazily-hub/lazily-js/reactive";

/** Any lazily derived/source handle. */
export type LazilyHandle<T = unknown> =
  | CellHandle<T>
  | SlotHandle<T>
  | SignalHandle<T>;

/**
 * Read any lazily handle from `ctx`, dispatching on handle class. Inside a
 * tracked computation the read also registers the dependency edge.
 */
export declare function readHandle<T>(
  ctx: Context,
  handle: LazilyHandle<T>,
): T;

/**
 * Subscribe an `onChange` callback to a lazily handle. Returns an unsubscribe
 * that disposes the underlying effect. Equality-guard semantics come from the
 * handle (memo/signal suppress equal recomputes; slot/computed always propagate).
 */
export declare function createLazilySubscription(
  ctx: Context,
  handle: LazilyHandle,
  onChange: () => void,
): () => void;
