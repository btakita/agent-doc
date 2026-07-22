# lazily-html

Isomorphic fine-grained HTML renderer on [`@lazily-hub/lazily-js`][lazily-js] —
browser **and** server (SSR via `renderToString`) from one package.
Preact/React-compatible API surface (`createElement`/`h`, `Fragment`,
`Component`, hooks, `createContext`, `forwardRef`, `memo`, `render`,
`renderToString`) with **Solid-style run-once components**: `setState` writes a
lazily cell and only the DOM expressions that read it patch — no virtual-DOM
diffing of the component subtree.

Named `lazily-html` (not `lazily-ui`) because the `html` carves the renderer-
target namespace: sibling packages like `lazily-slint`, `lazily-tui`, etc. can
sit alongside this one as alternative UI surfaces on the same lazily reactive
core.

## Two renderers, one reactive core, both isomorphic

Both share the same lazily bridge (`owner.js`) and the same Preact-compat layer
(`Component`, hooks, `createContext`, `forwardRef`, `memo`):

| Import | Renderer | Style | Size | Use when |
|---|---|---|---|---|
| `@lazily-hub/lazily-html` (default) | **lite** | relementjs/VanJS-style (`tags.div(...)`) + Preact `h()`, + `renderToString` SSR | ~4 KB browser / ~2 KB SSR | the common case; tiny widgets, isomorphic, embedded |
| `@lazily-hub/lazily-html/full` | **full** | dom-expressions hyperscript + Preact `h()` | ~6.5 KB | big keyed lists (LIS reconcile), event delegation, hydration (full SSR via dom-expressions `server.js` is a planned follow-up) |

### Why two?

The lite renderer is ~150 lines on the lazily bridge, modeled on
[relementjs][relementjs] (itself a VanJS fork). It detects function-valued
props/children and wraps each in a lazily effect that patches a single DOM
attribute, text node, or child. It has **no keyed-list reconcile and no event
delegation** — fine for ~90% of UIs, marginal only at thousands of rows.

The full renderer [vendors][vendor] [dom-expressions][dom-expressions] (Ryan
Carniato, MIT) — the canonical fine-grained DOM runtime that powers Solid — with
its bare `rxcore` import rewritten to `owner.js`. That buys keyed LIS list
reconcile, document-level event delegation, full SVG/attribute/property tables,
custom-element handling, and hydration. Same Preact API on top.

## Option-A semantics — read this

This is a fine-grained renderer, **not** a VDOM renderer. The one semantic
deviation from stock Preact/React:

> `useState` / `useReducer` return a Solid-style **accessor** as the first
> element, not a snapshot value.

```js
const [count, setCount] = useState(0);
// count is () => number, NOT number.
```

Why: in a no-compile hyperscript renderer, the only way JSX `{count}` can be
reactive is if `count` is a function the renderer detects and binds in an effect.
Reading the accessor inside any tracked effect registers the lazily dependency
edge, so `setCount(v)` patches exactly the DOM expressions that read it. The
component body runs **once per mount** (#lzdomnoTrackBody). `useSignal` is
provided as a Solid-familiar alias.

For class components, `this.state` is a Proxy whose property reads hit lazily
cells: `this.state.count` inside `() => this.state.count` is reactive, and
`this.setState({count: v})` patches the bound expressions without re-running
`render()`. `this.props` auto-unwraps reactive accessor props on read.

## Quick start

### Lite renderer (default)

```js
import { h, useState, render } from "@lazily-hub/lazily-html";

function Counter() {
  const [count, setCount] = useState(0);
  return h("button",
    { onClick: () => setCount(count() + 1) },
    () => `clicked ${count()} times`,
  );
}

render(h(Counter), document.getElementById("app"));
```

relementjs-style alternative (same renderer):

```js
import { tags, useState, render } from "@lazily-hub/lazily-html";
const { button } = tags;

function Counter() {
  const [count, setCount] = useState(0);
  return button({ onClick: () => setCount(count() + 1) }, () => `clicked ${count()} times`);
}
```

### Full renderer (dom-expressions-backed)

```js
import { h, useState, render } from "@lazily-hub/lazily-html/full";

function Counter() {
  const [count, setCount] = useState(0);
  return h("button",
    { onClick: () => setCount(count() + 1) },
    () => `clicked ${count()} times`,
  );
}

render(h(Counter), document.getElementById("app"));
```

The full `h` is hyper-dom-expressions hyperscript (CSS-selector-capable:
`h("div.box#main", ...)`) and the `client` export exposes the dom-expressions
utilities (`insert`, `spread`, `template`, `delegateEvents`, `For`/`Show`
helpers) for Solid-style patterns.

### Server-side rendering (isomorphic)

The same component tree renders to an HTML string on the server — no DOM global
required. Reactive accessor expressions evaluate to their initial value; effect
hooks (`useEffect`/`useLayoutEffect`) no-op on the server.

```js
import { h, useState, renderToString } from "@lazily-hub/lazily-html";

function Counter() {
  const [count] = useState(0);
  return h("button", null, `clicked ${count()} times`);  // or () => `...` for the accessor pattern
}

const html = renderToString(h(Counter));
// => "<button>clicked 0 times</button>"
```

`useState` / `useMemo` / `useContext` / `createContext` / `Component` all work
identically on browser and server, so SSR output matches the browser's first
paint. `renderToStaticMarkup` is an alias that also enforces the server render
mode around the call.

### Hydration via lazily sync (resumable, ~526 B)

Hydration here is **a sync operation, not a re-execution.** The server emits HTML
with `data-lzh` anchor attributes alongside a snapshot of the reactive values at
those anchors. The client applies the snapshot to a `SyncedView` (its reactive
state arrives authoritative, not recomputed), then a hyop-shape walker
(`querySelectorAll('[data-lzh]')`) binds each anchor to the synced cell. The
component tree never re-runs on the client.

```js
// Server
import { h, renderToStreamable } from "@lazily-hub/lazily-html";
const Counter = () => h("button", { id: "c" }, () => `count: ${0}`);
const { html, snapshot } = renderToStreamable(h(Counter));
// → { html: `<button id="c" data-lzh="a1">count: 0</button>`,
//     snapshot: { version: 1, anchors: { a1: "count: 0" } } }

// Client
import { hydrate, SyncedView } from "@lazily-hub/lazily-html";
const root = document.getElementById("app");
root.innerHTML = html;
const view = new SyncedView();
view.applySnapshot(snapshot);          // authoritative state — no re-derivation
hydrate(root, view);                   // hyop-shape anchor walk (526 B)
// Updates flow straight through the synced graph:
document.getElementById("c").addEventListener("click", () => {
  view.set("a1", `count: ${next()}`);  // patches only this button's text node
});
```

This repurposes lazily's sync machinery — the `SyncedView.applySnapshot` /
`applyDelta` / `get` / `set` surface mirrors lazily's `GraphView.applySnapshot` /
`applyDelta`. v0.1 ships **static-snapshot hydration**: anchor-scoped values,
sole-function-child text bindings, and function-valued attribute bindings. The
upgrade path is live `Delta` streaming (same `SyncedView.applyDelta` API,
lazily Delta wire format, capability-filtered via `Snapshot.filterReadable`) —
hydration then becomes "the first sync op," with every subsequent server push a
`Delta` through the same pipe.

Anchor descriptor format (space-separated in `data-lzh`):
- `<id>` → text binding (`el.textContent` tracks the cell)
- `a:<attr>:<id>` → attribute binding (`el.setAttribute(attr, ...)` tracks the cell)

## Architecture

```
@lazily-hub/lazily-js/reactive  ← lazily Context (Cell/Slot/Signal/Effect)
            │
            ▼
       owner.js                 ← rxcore bridge: root / effect / memo / source / onCleanup
            │
   ┌────────┴────────┐
   ▼                 ▼
 lite.js          full/            ← host-element renderers
 (relementjs)     (dom-expressions client + reconcile + constants + hyper)
   │                 │
   └────────┬────────┘
            ▼
   mount.js + instance.js + component.js + hooks.js + context.js
            ← shared Preact-compat layer (renderer-agnostic)
```

## Size

<!-- size-limits:start -->

Generated for package `@lazily-hub/lazily-html` version `0.1.0`. Every entry is **minified + brotlied, tree-shaken to the named import** (`size-limit` + esbuild, the same pipeline Webpack/Rollup/Vite apply via `"sideEffects": false`).

Refresh command:

```bash
npm run build            # regenerates this table as part of every build
npm run test:size        # gate: fails CI if any entry exceeds its budget
```

| Import | Size | Budget |
|---|---:|---:|
| lite: h + Component + hooks + render | 3.96 KB ✓ | 4.50 KB |
| lite: tags (relementjs-style) | 3.29 KB ✓ | 3.80 KB |
| server: renderToString (SSR) | 2.17 KB ✓ | 2.50 KB |
| hydrate (client) + SyncedView | 543 B ✓ | 1.50 KB |
| owner bridge (lazily adapter) | 384 B ✓ | 1.50 KB |
| full: h + Component + hooks + render (dom-expressions-backed) | 6.46 KB ✓ | 7.50 KB |

<!-- size-limits:end -->

## Vendored third-party code

`src/full/client.js`, `src/full/reconcile.js`, `src/full/constants.js`, and
`src/full/hyper.js` are vendored from [dom-expressions][dom-expressions] and
[hyper-dom-expressions][hyper] (© Ryan Carniato, MIT). The only change from
upstream is rewriting the bare `rxcore` import to `../owner.js` and adding `.js`
extensions for Node ESM. Diff against upstream before upgrading.

## License

MIT.

[lazily-js]: https://github.com/lazily-hub/lazily-js
[relementjs]: https://github.com/relementjs/relementjs
[dom-expressions]: https://github.com/ryansolid/dom-expressions
[hyper]: https://github.com/ryansolid/dom-expressions/tree/main/packages/hyper-dom-expressions
[vendor]: #vendored-third-party-code
