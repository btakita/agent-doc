// @lazily-hub/lazily-html/full — dom-expressions-backed renderer.
//
// Vendored dom-expressions runtime (keyed LIS reconcile, event delegation,
// hydration, full SVG/attribute/property tables) on lazily's owner bridge.
// Provides:
//   - `h`            — hyper-dom-expressions hyperscript (Solid-style, native)
//   - `createElement`/`h$/`Fragment`/`render` — Preact-shape VNode API
//   - `Component`, hooks, `createContext`, `forwardRef`, `memo` — shared with lite
//   - `client`       — dom-expressions utilities (insert/spread/template/delegateEvents/...)
//
// Both `h` and `createElement` produce reactive DOM on the same lazily core.

import * as client from "./client.js";
import { createHyperScript, $ELEMENT } from "./hyper.js";
import { root, getOwner, untrack } from "../owner.js";
import { withInstance } from "../instance.js";
import {
  isContextProvider,
  attachContextCell,
  updateContextCell,
} from "../context.js";
import { isClassComponent, mountClassInstance } from "../component.js";
import {
  createElement,
  h as hLite,
  Fragment,
  isValidElement,
  cloneElement,
} from "../create-element.js";
import { makePropsProxy } from "../mount.js";
import { effect } from "../owner.js";

// -- component mounting (shared owner/instance setup, host via hyper) --------

function createComponentFull(Comp, props) {
  return root(() => {
    const owner = getOwner();

    if (isContextProvider(Comp)) {
      const ctx = Comp._context;
      const initial =
        typeof props.value === "function" ? props.value() : props.value;
      attachContextCell(owner, ctx, initial);
      if (typeof props.value === "function") {
        effect(() => updateContextCell(owner, ctx, props.value()));
      }
      return mountFull(props.children, false);
    }

    if (isClassComponent(Comp)) {
      return mountClassInstance(Comp, props, false, owner, mountFull);
    }

    const instance = {
      _hooks: [],
      _hookIndex: 0,
      props,
      _owner: owner,
      _isFunctionInstance: true,
    };
    owner.component = instance;
    let rendered;
    withInstance(instance, () => {
      try {
        rendered = Comp(makePropsProxy(props));
      } catch (err) {
        console.error(
          `lazily-html/full: ${(Comp.displayName || Comp.name) || "Component"} threw:`,
          err,
        );
        rendered = null;
      }
    });
    return mountFull(rendered, false);
  });
}

// -- VNode → DOM ------------------------------------------------------------

function mountFull(value, isSVG) {
  if (value == null || value === false || value === true) return null;
  const t = typeof value;
  if (t === "string") return document.createTextNode(value);
  if (t === "number" || t === "bigint") return document.createTextNode(String(value));
  if (t === "function") {
    // hyper-dom-expressions thunks carry the $ELEMENT marker; calling them
    // builds the element. A bare accessor (no marker) is a reactive read.
    if (value[$ELEMENT]) return mountFull(value(), isSVG);
    // bare accessor at top level — anchor + reactive insert
    const marker = document.createTextNode("");
    let frag = marker;
    effect(() => {
      let n = value();
      while (typeof n === "function") n = n();
      n = mountFull(n, isSVG);
      if (n == null) {
        if (frag !== marker && frag.parentNode) {
          frag.replaceWith(marker);
          frag = marker;
        }
        return;
      }
      const next = Array.isArray(n) ? n : [n];
      if (frag !== marker) {
        frag.replaceWith(...next);
      } else {
        marker.before(...next);
      }
      frag = next.length === 1 ? next[0] : next;
    });
    return marker;
  }
  if (Array.isArray(value)) {
    const out = [];
    for (const v of value) {
      const n = mountFull(v, isSVG);
      if (n == null) continue;
      if (Array.isArray(n)) out.push(...n);
      else out.push(n);
    }
    return out;
  }
  if (value.__v) {
    if (typeof value.type === "function") {
      return createComponentFull(value.type, value.props || {});
    }
    return mountHostFull(value, isSVG);
  }
  if (value.nodeType) return value;
  return document.createTextNode(String(value));
}

function mountHostFull(vnode, isSVG) {
  const { type, props } = vnode;
  const propsCopy = {};
  let children = null;
  if (props) {
    for (const k in props) {
      if (k === "children") children = props[k];
      else if (k === "key") continue;
      else propsCopy[k] = props[k]; // ref stays — client.spread handles it
    }
  }
  const args = [type, propsCopy];
  if (children != null && children !== false && children !== true) {
    const arr = Array.isArray(children) ? children : [children];
    for (const c of arr) {
      if (c == null || c === false || c === true) continue;
      args.push(childToHyper(c, isSVG));
    }
  }
  const build = h(...args);
  return build();
}

function childToHyper(c, isSVG) {
  if (c && c.__v) return mountFull(c, isSVG);
  if (typeof c === "function") {
    // accessor returning a vnode/value — mount on each evaluation so hyper's
    // insert/reconcile sees real nodes (LIS reconcile applies when nodes are
    // stable across evaluations; for fully reactive lists use client-side For).
    return () => {
      let n = c();
      while (typeof n === "function") n = n();
      return mountFull(n, isSVG);
    };
  }
  return c; // string / number / Node — hyper handles directly
}

// -- hyperscript instance ---------------------------------------------------

export const h = createHyperScript({
  insert: client.insert,
  spread: client.spread,
  assign: client.assign,
  dynamicProperty: client.dynamicProperty,
  SVGElements: client.SVGElements,
  createComponent: createComponentFull,
});

export { createElement, Fragment, isValidElement, cloneElement, mountFull as mount, client };

export function render(vnode, parent) {
  return root(() => {
    const node = mountFull(vnode, false);
    if (node != null) {
      if (Array.isArray(node)) parent.append(...node);
      else parent.append(node);
    }
    return node;
  });
}

// Shared Preact-compat surface (identical to lite).
export { Component, isClassComponent } from "../component.js";
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
} from "../hooks.js";
export { createContext, forwardRef, memo } from "../context.js";
export {
  root,
  effect,
  source,
  onCleanup,
  untrack,
  getOwner,
  runWithOwner,
  mergeProps,
} from "../owner.js";

// dom-expressions utilities for power users (template, delegateEvents, etc.)
export const dom = client;
