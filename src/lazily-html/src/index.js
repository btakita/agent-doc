// @lazily-hub/lazily-html — fine-grained DOM renderer on lazily-js.
//
// DEFAULT ENTRY = lite renderer (relementjs/VanJS-style, ~1 KB). For the
// dom-expressions-backed full renderer (keyed-list reconcile, event delegation,
// hydration, full SVG/prop tables), import from `@lazily-hub/lazily-html/full`.
//
// Both renderers share the same Preact-shape API (createElement/h, Fragment,
// Component, hooks, createContext, forwardRef, memo, render) and the same lazily
// reactive core. See README for the Option-A semantic deviation: `useState`
// returns a Solid-style accessor `[getter, setter]` so JSX `{count}` is
// reactive without a compiler.

export {
  createElement,
  h,
  Fragment,
  isValidElement,
  cloneElement,
  render,
  mount,
  tags,
  tagsNS,
  attach,
  fragment,
  raw,
  setProp,
} from "./lite.js";

export { renderToString, renderToStaticMarkup, renderToStreamable } from "./server.js";
export { hydrate } from "./hydrate.js";
export { SyncedView } from "./synced-view.js";

export { Component } from "./component.js";
export { isClassComponent } from "./component.js";

export {
  useState,
  useSignal,
  useReducer,
  useEffect,
  useLayoutEffect,
  useRef,
  useMemo,
  useCallback,
  useContext,
  useId,
  useDebugValue,
  withInstance,
  getCurrentInstance,
} from "./hooks.js";

export {
  createContext,
  forwardRef,
  memo,
} from "./context.js";

export {
  root,
  effect,
  memo as reactiveMemo,
  source,
  onCleanup,
  untrack,
  getOwner,
  runWithOwner,
  getReactiveContext,
  setReactiveContext,
  mergeProps,
} from "./owner.js";
