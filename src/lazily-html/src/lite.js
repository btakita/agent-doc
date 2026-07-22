// Lite renderer — relementjs/VanJS-style fine-grained DOM on lazily.
//
// ~150 lines on top of owner.js (the lazily bridge). Reactive values are
// function-valued props/children: the renderer detects them and wraps each in
// a lazily effect that patches the single DOM attribute, text node, or child.
// No keyed-list LIS reconcile, no event delegation — each handler is its own
// listener (fine for the vast majority of UIs). For apps that need reconcile
// + delegation + hydration, use `lazily-html/full` (dom-expressions-backed).
//
// Exposes two construction styles over the SAME renderer:
//   - Preact-shape:  h("div", {class: "x"}, h("span", null, "hi"))
//   - relementjs-shape: tags.div({class: "x"}, tags.span("hi"))

import { makeMount } from "./mount.js";
import { root, effect, onCleanup } from "./owner.js";
import {
  createElement,
  h,
  Fragment,
  isValidElement,
  cloneElement,
} from "./create-element.js";

const SVG_NS = "http://www.w3.org/2000/svg";
const SVG_TAGS = new Set([
  "svg", "g", "defs", "symbol", "use", "path", "circle", "ellipse", "rect",
  "line", "polyline", "polygon", "text", "tspan", "textPath", "linearGradient",
  "radialGradient", "stop", "filter", "mask", "marker", "pattern", "image",
  "foreignObject", "desc", "title", "metadata", "clipPath", "view", "switch",
  "animate", "animateMotion", "animateTransform", "set", "feBlend",
  "feColorMatrix", "feComponentTransfer", "feComposite", "feGaussianBlur",
  "feMerge", "feMergeNode", "feOffset", "feFlood", "feTile", "feTurbulence",
]);

function isSVGTag(tag) {
  return tag === "svg" || SVG_TAGS.has(tag);
}

function hostElement(tag, isSVG) {
  return isSVG
    ? document.createElementNS(SVG_NS, tag)
    : document.createElement(tag);
}

// #lzdomattrheuristic: a name is an attribute (not a property) when no setter
// exists anywhere on the DOM prototype chain. Mirrors relementjs's
// PropertyDescriptor_ walk. Cached per (localName,key) so the walk is amortized
// to once per tag+key across the whole page lifetime.
const attrDecisionCache = Object.create(null);
function decideAttr(dom, key) {
  const cacheKey = dom.localName + "," + key;
  const cached = attrDecisionCache[cacheKey];
  if (cached !== undefined) return cached;
  let proto = Object.getPrototypeOf(dom);
  let hasSetter = false;
  while (proto) {
    const pd = Object.getOwnPropertyDescriptor(proto, key);
    if (pd && "set" in pd && pd.set) { hasSetter = true; break; }
    if (pd && pd.writable && typeof pd.value !== "function") { hasSetter = true; break; }
    proto = Object.getPrototypeOf(proto);
  }
  attrDecisionCache[cacheKey] = !hasSetter;
  return !hasSetter;
}

export function setProp(dom, key, value, isSVG) {
  if (key === "key" || key === "ref" || key === "children") return;

  if (key === "style") {
    if (value == null || value === false) { dom.removeAttribute("style"); return; }
    if (typeof value === "string") { dom.style.cssText = value; return; }
    if (typeof value === "object") {
      if (dom.style.cssText) dom.style.cssText = "";
      for (const k in value) {
        const sv = value[k];
        sv == null ? dom.style.removeProperty(k) : (dom.style[k] = sv);
      }
      return;
    }
  }

  if (key === "className" || key === "class") {
    if (value == null || value === false) dom.removeAttribute("class");
    else dom.setAttribute("class", String(value));
    return;
  }

  if (key === "classList" && value && typeof value === "object") {
    for (const k in value) dom.classList.toggle(k, !!value[k]);
    return;
  }

  if (key.startsWith("onCapture") && typeof value === "function") {
    dom.addEventListener(key.slice(8).toLowerCase(), value, true);
    return;
  }
  if (key.startsWith("on") && (typeof value === "function" || value == null)) {
    const ev = key.slice(2).toLowerCase();
    if (value == null) { /* noop */ }
    else dom.addEventListener(ev, value);
    return;
  }
  if (key.startsWith("on") && typeof value === "object" && value) {
    // Preact-style handler object {handleEvent} or AddEventListenerOptions
    const ev = key.slice(2).toLowerCase();
    dom.addEventListener(ev, value);
    return;
  }

  if (key.startsWith("attr:")) {
    const name = key.slice(5);
    if (value == null || value === false) dom.removeAttribute(name);
    else dom.setAttribute(name, value === true ? "" : String(value));
    return;
  }
  if (key.startsWith("prop:")) {
    dom[key.slice(5)] = value;
    return;
  }
  if (key.startsWith("bool:")) {
    const name = key.slice(5);
    value ? dom.setAttribute(name, "") : dom.removeAttribute(name);
    return;
  }

  if (key.includes("-") && key !== "data-") {
    // Custom attributes, data-*, aria-* — always attributes.
    if (value == null || value === false) dom.removeAttribute(key);
    else dom.setAttribute(key, value === true ? "" : String(value));
    return;
  }

  if (decideAttr(dom, key)) {
    if (value == null || value === false) dom.removeAttribute(key);
    else dom.setAttribute(key, value === true ? "" : String(value));
  } else {
    try {
      dom[key] = value == null ? "" : value;
    } catch (_) {
      if (value == null || value === false) dom.removeAttribute(key);
      else dom.setAttribute(key, value === true ? "" : String(value));
    }
  }
}

function assignProps(node, props, isSVG, vnode) {
  for (const key in props) {
    if (key === "ref" || key === "key" || key === "children") continue;
    const v = props[key];
    // #lzdomreactiveprop: function-valued non-on props are reactive accessors.
    if (typeof v === "function" && !key.startsWith("on")) {
      effect(() => setProp(node, key, v(), isSVG));
    } else {
      setProp(node, key, v, isSVG);
    }
  }
}

function insertChild(parent, child, marker, isSVG) {
  if (child == null || child === false || child === true) return;
  const t = typeof child;
  if (t === "string") { parent.insertBefore(textNode(child), marker); return; }
  if (t === "number" || t === "bigint") { parent.insertBefore(textNode(String(child)), marker); return; }
  if (t === "function") {
    let cur = null; // Node | Node[] | null — the previously-inserted nodes
    effect(() => {
      let n = child();
      while (typeof n === "function") n = n();
      n = mountLite(n, isSVG);
      let nodes;
      if (n == null || n === false || n === true) nodes = [];
      else if (Array.isArray(n)) {
        nodes = [];
        for (const x of n) {
          if (x != null) (Array.isArray(x) ? nodes.push(...x) : nodes.push(x));
        }
      } else nodes = [n];
      if (cur) {
        const curArr = Array.isArray(cur) ? cur : [cur];
        if (nodes.length) {
          curArr[0].replaceWith(...nodes);
          for (let i = 1; i < curArr.length; i++) curArr[i].remove();
        } else {
          for (const c of curArr) c.remove();
        }
      } else if (nodes.length) {
        parent.insertBefore(nodes[0], marker);
        for (let i = 1; i < nodes.length; i++) parent.insertBefore(nodes[i], marker);
      }
      cur = nodes.length === 0 ? null : nodes.length === 1 ? nodes[0] : nodes;
    });
    return;
  }
  if (Array.isArray(child)) {
    for (let i = 0; i < child.length; i++) insertChild(parent, child[i], marker, isSVG);
    return;
  }
  if (child.__v) {
    parent.insertBefore(mountLite(child, isSVG), marker);
    return;
  }
  if (child.nodeType) {
    parent.insertBefore(child, marker);
    return;
  }
  parent.insertBefore(textNode(String(child)), marker);
}

function textNode(s) {
  return document.createTextNode(s);
}

function removeMany(cur) {
  if (Array.isArray(cur)) for (const n of cur) n.remove();
  else cur.remove();
}

const liteApi = { isSVGTag, hostElement, assignProps, insertChild };
const mountLite = makeMount(liteApi);

// -- Public render entry ---------------------------------------------------

export function render(vnode, parent) {
  return root(() => {
    const node = mountLite(vnode, false);
    if (node != null) {
      if (Array.isArray(node)) parent.append(...node);
      else parent.append(node);
    }
    return node;
  });
}

export { mountLite as mount };

// -- relementjs-style tags API ---------------------------------------------
//
// `tags.div({class: "x"}, tags.span("hi"), () => state.label)` — builds DOM
// directly. Function-valued props/children are reactive. camelCase props.

export function tagsNS(ns) {
  return new Proxy(function tagFactory(name, ...args) {
    const [params = {}, ...rest] =
      args.length && args[0] != null && typeof args[0] === "object" && !args[0].nodeType && !args[0].__v && !Array.isArray(args[0])
        ? args
        : [{}].concat(args);
    const dom = ns
      ? document.createElementNS(ns, name)
      : document.createElement(name);
    for (const key in params) {
      const v = params[key];
      if (typeof v === "function" && !key.startsWith("on")) {
        effect(() => setProp(dom, key, v(), !!ns));
      } else {
        setProp(dom, key, v, !!ns);
      }
    }
    return attach(dom, ...rest);
  }, {
    get: (tag, name) => tag.bind(undefined, name),
  });
}

export const tags = tagsNS("");

export function attach(dom, ...children) {
  const flat = children.flat(Infinity);
  const multi = flat.some((c) => typeof c === "function" || Array.isArray(c));
  if (multi) {
    const marker = document.createTextNode("");
    dom.appendChild(marker);
    for (const c of flat) insertChild(dom, c, marker, false);
    // keep marker for reactive children stability
  } else {
    for (const c of flat) {
      if (c == null || c === false || c === true) continue;
      const t = typeof c;
      if (t === "string" || t === "number" || t === "bigint") dom.appendChild(textNode(String(c)));
      else if (c.__v) dom.appendChild(mountLite(c, false));
      else if (c.nodeType) dom.appendChild(c);
      else dom.appendChild(textNode(String(c)));
    }
  }
  return dom;
}

export function fragment(...children) {
  const frag = document.createDocumentFragment();
  attach(frag, ...children);
  return frag;
}

// -- raw HTML (like relementjs raw_) ---------------------------------------

export function raw(html) {
  const tpl = document.createElement("template");
  const wrap = (v) => {
    tpl.innerHTML = typeof v === "function" ? v() : v;
    const frag = document.createDocumentFragment();
    frag.append(...tpl.content.childNodes);
    return frag;
  };
  if (typeof html === "function") {
    let cur = wrap(html());
    effect(() => {
      const next = wrap(html());
      cur.replaceWith(next);
      cur = next;
    });
    return cur;
  }
  return wrap(html);
}

export { createElement, h, Fragment, isValidElement, cloneElement, root, effect, onCleanup };
