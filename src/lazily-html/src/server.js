// Server-side rendering — renderToString.
//
// Mirrors the browser mount walker but emits an HTML string instead of DOM
// nodes. The SAME component tree runs (function + class components, hooks,
// context) so SSR output matches what the browser would produce on first
// paint. Reactive accessor expressions are evaluated ONCE for their initial
// value — server output is static HTML.
//
// Effect hooks (useEffect/useLayoutEffect) are no-ops on the server
// (#lzdomservernoop) — they're a browser-only concern. useState/useReducer
// still return [accessor, setter] pairs; reading the accessor yields the
// initial value.

import { root, getOwner } from "./owner.js";
import { withInstance, setRenderMode } from "./instance.js";
import {
  isContextProvider,
  attachContextCell,
} from "./context.js";
import { isClassComponent } from "./component.js";
import { makePropsProxy } from "./mount.js";

const VOID_ELEMENTS = new Set([
  "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
  "param", "source", "track", "wbr",
]);

const SVG_TAGS = new Set([
  "svg", "g", "defs", "symbol", "use", "path", "circle", "ellipse", "rect",
  "line", "polyline", "polygon", "text", "tspan", "textPath", "linearGradient",
  "radialGradient", "stop", "filter", "mask", "marker", "pattern", "image",
  "foreignObject", "desc", "title", "metadata", "clipPath", "view", "switch",
]);

const ATTR_ALIASES = { className: "class", class: "class", htmlFor: "for" };

// #lzhsyncanchor — anchor state for streamable SSR. When non-null (set by
// renderToStreamable), reactive function-valued props and sole-function children
// get a stable `data-lzh` anchor id; the evaluated initial value is recorded in
// ANCHOR_STATE.anchors. The client's `hydrate` walks `[data-lzh]` and binds each
// anchor to a synced cell — hydration = applySnapshot + anchor walk.
// Mirrors lazily's `GraphView.applySnapshot` shape; live updates will use
// `applyDelta` (lazily Delta wire) through the same SyncedView.
let ANCHOR_STATE = null;

function anchorFor(fnValue) {
  if (!ANCHOR_STATE) return null;
  const id = "a" + ++ANCHOR_STATE.nextId;
  const evaluated = unwrap(fnValue);
  ANCHOR_STATE.anchors[id] = evaluated == null ? "" : evaluated;
  return { id, evaluated };
}

function isSVGTag(tag) {
  return tag === "svg" || SVG_TAGS.has(tag);
}

function escapeHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function escapeAttr(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/"/g, "&quot;");
}

// Evaluate a possibly-accessor value to its current (initial) value.
function unwrap(v) {
  let cur = v;
  while (typeof cur === "function") cur = cur();
  return cur;
}

function styleToString(value) {
  if (value == null || value === false) return "";
  if (typeof value === "string") return value;
  if (typeof value === "object") {
    const parts = [];
    for (const k in value) {
      const v = unwrap(value[k]);
      if (v == null || v === false) continue;
      // camelCase → kebab-case
      const kebab = k.replace(/[A-Z]/g, (m) => "-" + m.toLowerCase());
      parts.push(`${kebab}:${v}`);
    }
    return parts.join(";");
  }
  return "";
}

function classListToString(value) {
  if (!value || typeof value !== "object") return "";
  const parts = [];
  for (const k in value) {
    if (unwrap(value[k])) parts.push(k);
  }
  return parts.join(" ");
}

function propToAttr(tagName, key, rawValue, isSVG) {
  if (key === "key" || key === "ref" || key === "children") return null;
  // Events do not render on the server.
  if (key.startsWith("on")) return null;

  const value = unwrap(rawValue);

  if (key === "style") {
    const s = styleToString(value);
    return s ? `style="${escapeAttr(s)}"` : null;
  }
  if (key === "classList") {
    const s = classListToString(value);
    return s ? `class="${escapeAttr(s)}"` : null;
  }

  let name = ATTR_ALIASES[key] || key;

  if (key.startsWith("attr:")) {
    name = key.slice(5);
    if (value == null || value === false) return null;
    return `${name}="${escapeAttr(value === true ? "" : value)}"`;
  }
  if (key.startsWith("bool:")) {
    name = key.slice(5);
    return value ? `${name}` : null;
  }
  if (key.startsWith("prop:")) {
    name = key.slice(5);
  }

  if (value == null || value === false) return null;
  if (value === true) return `${name}`;
  return `${name}="${escapeAttr(value)}"`;
}

function renderHostToString(vnode, isSVG) {
  const { type, props } = vnode;
  const svg = isSVG || isSVGTag(type);
  const lzhTokens = [];
  const anchoredAttrs = {}; // key -> evaluated value (replaces rawValue for emission)
  let textAnchor = null;

  if (props) {
    // Pass 1a: anchor reactive function-valued non-on props.
    for (const key in props) {
      const rawValue = props[key];
      if (
        ANCHOR_STATE &&
        typeof rawValue === "function" &&
        !key.startsWith("on") &&
        key !== "key" &&
        key !== "ref" &&
        key !== "children" &&
        key !== "style" &&
        key !== "classList"
      ) {
        const anchor = anchorFor(rawValue);
        if (anchor) {
          lzhTokens.push(`a:${ATTR_ALIASES[key] || key}:${anchor.id}`);
          anchoredAttrs[key] = anchor.evaluated;
        }
      }
    }
    // Pass 1b: anchor sole-function child as a text binding.
    if (ANCHOR_STATE && typeof props.children === "function") {
      textAnchor = anchorFor(props.children);
      if (textAnchor) lzhTokens.push(textAnchor.id);
    }
  }

  // Pass 2: emit opening tag with attributes + data-lzh.
  let out = "<" + type;
  if (props) {
    for (const key in props) {
      if (key === "children") continue;
      const value = key in anchoredAttrs ? anchoredAttrs[key] : props[key];
      const attr = propToAttr(type, key, value, svg);
      if (attr) out += " " + attr;
    }
  }
  if (lzhTokens.length) {
    out += ` data-lzh="${escapeAttr(lzhTokens.join(" "))}"`;
  }
  if (VOID_ELEMENTS.has(type)) return out + ">";

  // Pass 3: body.
  out += ">";
  let body = "";
  if (props && props.children != null && props.children !== false && props.children !== true) {
    if (textAnchor) {
      body = escapeHtml(textAnchor.evaluated == null ? "" : String(textAnchor.evaluated));
    } else {
      body = renderToString(props.children, svg);
    }
  }
  return out + body + "</" + type + ">";
}

function renderComponentToString(type, props, isSVG) {
  return root(() => {
    const owner = getOwner();

    if (isContextProvider(type)) {
      const ctx = type._context;
      attachContextCell(owner, ctx, unwrap(props.value));
      return renderToString(props.children, isSVG);
    }

    if (isClassComponent(type)) {
      const instance = { _hooks: [], _hookIndex: 0, props, _owner: owner };
      owner.component = instance;
      const inst = withInstance(instance, () => new type(makePropsProxy(props)));
      inst._owner = owner;
      inst._instance = instance;
      if (inst.state == null) inst.state = {};
      let rendered;
      withInstance(instance, () => {
        rendered = inst.render();
      });
      return renderToString(rendered, isSVG);
    }

    const instance = { _hooks: [], _hookIndex: 0, props, _owner: owner };
    owner.component = instance;
    let rendered;
    withInstance(instance, () => {
      rendered = type(makePropsProxy(props));
    });
    return renderToString(rendered, isSVG);
  });
}

export function renderToString(value, isSVG = false) {
  if (value == null || value === false || value === true) return "";
  const t = typeof value;
  if (t === "string") return escapeHtml(value);
  if (t === "number" || t === "bigint") return String(value);
  if (t === "function") {
    let n = value();
    while (typeof n === "function") n = n();
    return renderToString(n, isSVG);
  }
  if (Array.isArray(value)) {
    let out = "";
    for (const v of value) out += renderToString(v, isSVG);
    return out;
  }
  if (value.__v) {
    if (typeof value.type === "function") {
      return renderComponentToString(value.type, value.props || {}, isSVG);
    }
    return renderHostToString(value, isSVG);
  }
  return escapeHtml(String(value));
}

// Public entry — sets RENDER_MODE for the duration so effect hooks no-op.
export function renderToStaticMarkup(vnode) {
  setRenderMode("server");
  try {
    return renderToString(vnode, false);
  } finally {
    setRenderMode("browser");
  }
}

// Streamable SSR — emits HTML with `data-lzh` anchor attributes alongside a
// snapshot payload consumable by `SyncedView.applySnapshot`. The client's
// `hydrate(rootEl, view)` then walks `[data-lzh]` and binds each anchor to a
// synced cell — hydration becomes a sync operation, not a re-execution.
//
// v0.1 anchor scope: sole-function children (text binding) and function-valued
// non-on props (attribute binding). Mixed-array reactive children render as
// static (baked-in initial value); upgrade path is full lazily Delta streaming.
//
// The snapshot wire format is a lazily-html-specific sync fragment
// `{ version, anchors }`. It mirrors lazily's `GraphView.applySnapshot` shape;
// future versions upgrade to the full lazily `Snapshot` wire type (binary,
// capability-filtered) without changing the SyncedView API.
export function renderToStreamable(vnode) {
  setRenderMode("server");
  const prev = ANCHOR_STATE;
  ANCHOR_STATE = { anchors: {}, nextId: 0 };
  try {
    const html = renderToString(vnode, false);
    return {
      html,
      snapshot: { version: 1, anchors: ANCHOR_STATE.anchors },
    };
  } finally {
    ANCHOR_STATE = prev;
    setRenderMode("browser");
  }
}
