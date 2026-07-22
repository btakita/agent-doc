// hydrate — the client-side companion to renderToStreamable.
//
// Walks `[data-lzh]` anchors in the server-rendered DOM and binds each to the
// matching cell on a SyncedView via a lazily effect. Shape mirrors hyop
// (`querySelectorAll('[hyop]')` → registry fn), but each anchor reads from the
// synced reactive graph instead of looking up a handler function.
//
// Anchor descriptor format (in the `data-lzh` attribute, space-separated):
//   `<id>`           → text binding: el.textContent tracks view.get(id)
//   `a:<attr>:<id>`   → attribute binding: el.setAttribute(attr, view.get(id))
//
// The component tree is NOT re-executed on the client — that is the resumability
// win from "hydration = sync". Initial DOM values are already correct from the
// server HTML; the bindings exist only to keep them live as cells change.

import { root, effect, setReactiveContext } from "./owner.js";

export function hydrate(rootEl, view) {
  setReactiveContext(view.ctx);
  return root(() => {
    const els = rootEl.querySelectorAll("[data-lzh]");
    for (let i = 0; i < els.length; i++) {
      const el = els[i];
      const spec = el.getAttribute("data-lzh") || "";
      const tokens = spec.split(/\s+/);
      for (let j = 0; j < tokens.length; j++) {
        const tok = tokens[j];
        if (!tok) continue;
        if (tok.charAt(0) === "a" && tok.charAt(1) === ":") {
          // a:<attr>:<id>
          const rest = tok.slice(2);
          const sep = rest.indexOf(":");
          if (sep === -1) continue;
          const attr = rest.slice(0, sep);
          const id = rest.slice(sep + 1);
          bindAttr(el, attr, id, view);
        } else {
          bindText(el, tok, view);
        }
      }
    }
  });
}

function bindText(el, id, view) {
  effect(() => {
    const v = view.get(id);
    el.textContent = v == null ? "" : String(v);
  });
}

function bindAttr(el, attr, id, view) {
  effect(() => {
    const v = view.get(id);
    if (v == null || v === false) el.removeAttribute(attr);
    else if (v === true) el.setAttribute(attr, "");
    else el.setAttribute(attr, String(v));
  });
}
