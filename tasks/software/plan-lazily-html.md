# Plan: lazily-html remediation (`#lzhtml`)

**Goal:** take `src/lazily-html` from "untracked prototype whose headline claims
are unimplemented" to a releasable, honestly-described fine-grained HTML renderer
on lazily-js — with a real disposal model, correct Preact-compat semantics where
claimed, and a sync path that actually interoperates with a lazily server.

**Owner:** `src/lazily-html` (currently **not a git repo** — `?? src/lazily-html/`).
**Depends on:** `src/lazily-js` (0.24.0), `src/lazily-react` (positioning).
**Review date:** 2026-07-18. Findings below were reproduced by running the
library under linkedom, not inferred from reading.

## Current state — read this before touching anything

`npm test` is **34/34 green** (~78ms) while both headline claims are
unimplemented. The suite asserts happy paths only and never asserts a negative
(nothing recomputed, nothing leaked, nothing re-mounted, an error thrown, a
cleanup fired). Treat the green suite as **no signal**.

lazily-html touches lazily-js at exactly one point: `createContext` from
`@lazily-hub/lazily-js/reactive` (`src/owner.js:19`, `src/synced-view.js:16`).
Ownership, disposal, keyed reconcile, batching, and untrack are all hand-rolled
on top of raw `ctx.cell/memo/effect/get/set`. The reactive-graph usage itself is
legal 0.23 API — no stale `Signal`/`Cell`-reader misuse. **Every defect below is
in the layer lazily-html built for itself.**

Two things are worth keeping as-is: `.size-limit.json` + `scripts/update-size-limits.mjs`
(genuinely good infra that lazily-react lacks), and the renderer-target naming
rationale at `README.md:11-14`.

---

## Phase 0 — Infra blockers (`#lzhtml-infra`)

Nothing else can ship until these land. No code changes.

- [ ] `git init`; add remote `git@github.com:lazily-hub/lazily-html.git` —
      `package.json:6-9` already claims this URL.
- [ ] Add `.gitignore` (`node_modules/`) **before** the first commit, or it
      ingests `node_modules/`.
- [ ] Register as a submodule in `.gitmodules` (mirror the lazily-react entry at
      `.gitmodules:187-189`).
- [ ] Add `files: ["src", "README.md"]` and
      `publishConfig: {access: "public", provenance: false}`. `@lazily-hub/*` is
      **scoped** → defaults to restricted → publish fails or goes private
      without this. Copy lazily-react's convention (`package.json:28-31`, `:49-52`),
      not lazily-js's `.npmignore` + CLI-flag approach.
- [ ] Add `AGENTS.md`; symlink `CLAUDE.md` → it (repo convention).
- [ ] Add `Makefile` with a `check:` target (`npm run build && npm test`) so
      `make check` has an entry point.
- [ ] Add `LICENSE` (MIT per `package.json`). Note: absent in lazily-js and
      lazily-react too — fix all three.
- [ ] Add `.github/workflows/release.yml` modeled on lazily-js's: `v*` tag +
      `workflow_dispatch`, per-ref concurrency, npm OIDC trusted publishing
      (`id-token: write`), Node 24 with bundled npm 11 (do **not** upgrade npm
      globally — drops `sigstore`, see lazily-js `release.yml:42-46`),
      `npm publish --provenance` with idempotent already-published fallback.
      Requires a one-time manual first publish + Trusted Publisher config on npm.

---

## Phase 1 — The root bug: the owner tree is not a tree (`#lzhtml-owner`)

**This is one bug with two halves, and it is the single highest-leverage fix.**
Fixing it dissolves most of Phase 2.

- [ ] **1a. `root()` never registers children.** `src/owner.js:58-98` builds
      `{parent, effects, cleanups}` and a `dispose` that walks only its *own*
      lists. It never pushes `newOwner` onto `CURRENT_OWNER`'s cleanups, so
      disposal cannot cascade. Add a `children` array; register each child owner
      on its parent.

- [ ] **1b. `effect()` never re-enters its owner on re-run.** `src/owner.js:107-121`
      wraps `fn` in `ctx.effect(...)` without capturing `owner` and re-entering
      via the existing `runWithOwner` (`owner.js:46-54`). `CURRENT_OWNER` is a
      module global mutated only during the synchronous initial mount walk — so
      when lazily flushes an effect from an event handler, it is `null`.
      Verified: `2nd owner.parent: null`.

      Two consequences, the second of which is easy to miss:
      - Effects created during an update attach to an ephemeral scope —
        `owner.js:109-111` runs `root(() => effect(...))` and **discards** the
        returned `dispose`. Permanently unreachable.
      - **Context is silently wrong after the first update.**
        `readContextAccessor` (`src/context.js:63-72`) walks `owner.parent`,
        hits `null` immediately, and falls back to `ctx._defaultValue` instead
        of the Provider's value.

- [ ] **1c.** Dispose child scopes *before* an effect body re-runs.
- [ ] **1d.** `owner.js:109-111`'s ownerless `root()` branch should throw or
      take an explicit owner rather than silently leak a scope.

**Falls out of this fix:** disposal cascade, `componentWillUnmount`
(`component.js:167-176`) and `useEffect` teardown (`hooks.js:93-104`) becoming
reachable at all, detached-subtree effects (verified:
`runs mount/remove/after-write: 1 1 2 | leaked effect fired: true`), and half of
the cell/slot leak.

- [ ] **1e. `useEffect` teardown is broken independently.** `src/hooks.js:99-103`
      calls `getOwner()` at *cleanup* time, not capture time. At disposal
      `CURRENT_OWNER` is `null` → `null.ctx` throws a `TypeError` swallowed by
      the bare `catch (_) {}`. `owner` is already in scope at line 78 — close
      over it.

---

## Phase 2 — Cheap, visible correctness bugs (`#lzhtml-correct`)

Independently shippable. Do these alongside Phase 1.

- [ ] **2a. DOM corruption from the second update onward.** `src/mount.js:44-54`:
      the text-node branch runs `cur.replaceWith(...nodes); nodes.length = 0;`
      then the return expression reads the **mutated** array and yields `null`.
      Next run takes `marker.before(...nodes)` and inserts alongside the stale
      node. Verified: `"0"` → `"1"` → `"12"` → `"13"`. Any top-level `{count}`
      is corrupt. **One-line fix** (compute the return before mutating).
      Note `lite.js:159-192` implements the same path *without* the bug — the
      two have diverged; consider converging them.

- [ ] **2b. No batching anywhere.** `grep -rn "batch" src/` → zero hits, despite
      `ctx.batch` existing since 0.23 (`lazily-js/src/reactive.js:386`).
      `component.js:48-62` writes each patched key separately and lazily flushes
      synchronously per write (`reactive.js:357-384`). Verified: one logical
      `setState({a:2,b:2})` produces effect runs `[[1,1],[2,1],[2,2]]` — the
      `[2,1]` frame is a real glitch, a painted state that never logically
      existed. Wrap `setState` in `ctx.batch`.

- [ ] **2c. `makePropsProxy` invokes any non-`on*` function prop on read.**
      `src/mount.js:222-243` duck-types `typeof v === "function"`, which cannot
      distinguish a lazily accessor from a callback. This is the **root cause**
      of the forwardRef breakage, and it also destroys render props, comparators,
      and formatters. Verified: `forwardRef` ref arg is `undefined`, callback
      refs never fire, a render prop is invoked with 0 args and replaced by its
      return value. **The fix already exists in-tree:** `markAccessor`
      (`owner.js:216-219`) is used by nothing. Tag accessors; stop duck-typing.
      This also makes `mergeProps`' `_isAccessor` check (`owner.js:188`) live.

- [ ] **2d. `renderToString` never sets server mode.** `src/server.js:237` —
      only `renderToStaticMarkup:263` and `renderToStreamable:285` call
      `setRenderMode("server")`. So `renderToString` runs effects and throws on
      `document` access. This is the entry the README demonstrates
      (`README.md:123-131`), and it falsifies the claim at `README.md:120` that
      effect hooks no-op on the server.

- [ ] **2e. Reactive `raw()` throws on first update.** `src/lite.js:295-312`:
      `cur` is a `DocumentFragment`, which doesn't implement `ChildNode` (no
      `replaceWith`) and, once appended, is empty with no `parentNode`.
      Verified: `TypeError: cur.replaceWith is not a function`.

- [ ] **2f. `componentDidMount` fires on a detached node.**
      `src/component.js:157-165` calls it immediately after `mount(rendered)`,
      before the caller inserts into the document. Breaks ref measurement,
      `focus()`, `IntersectionObserver`. React/Preact guarantee attachment.

- [ ] **2g. `dangerouslySetInnerHTML` renders as a literal attribute.**
      `src/lite.js:66-144` has no case for it →
      `<div dangerouslySetInnerHTML="[object Object]">`. Content silently vanishes.

- [ ] **2h. Event name mapping is naive lowercasing.** `src/lite.js:98`
      `key.slice(2).toLowerCase()` → `onDoubleClick` becomes `"doubleclick"`
      (real event: `dblclick`). Every camelCase-compound React event name is
      broken identically.

- [ ] **2i. `decideAttr` cache key collides across namespaces.**
      `src/lite.js:50-51` keys on `localName + "," + key`. `a`, `title`,
      `script`, `style`, `font` exist in both HTML and SVG with different
      property/attribute semantics. Whichever renders first poisons the cache.

- [ ] **2j. Post-mount `this.state = {...}` silently kills reactivity.**
      `src/component.js:38-47` replaces every `source` cell wholesale; existing
      DOM effects stay subscribed to orphaned cells and stop updating.

---

## Phase 3 — Disposal API + real tests (`#lzhtml-dispose`)

Without this, Phase 1 regresses silently.

- [ ] **3a. `render()` must return a disposer.** `src/lite.js:223-232` and
      `src/full/index.js:184` both `return root(() => {...; return node})` —
      returning the **DOM node** and discarding `dispose`. `index.js:13-70`
      exports no `unmount`/`dispose`. There is no way for a consumer to tear
      down a rendered tree.
- [ ] **3b. Rewrite the vacuous test.** `test/lite.test.js:84-97` is titled
      *"useEffect runs once on mount and disposes on unmount"*, assigns
      `dispose = render(...)` (a DOM node), **never calls it**, and asserts only
      `log.includes("mount")`. It asserts neither "once" nor "disposes". This
      single test is why Phase 1 went unnoticed.
- [ ] **3c. Attach cell/slot handles to owners and dispose them.**
      `owner.js:135-143` (`source`) and `:125-131` (`memo`) allocate handles and
      push them to nothing. Requires the 0.24 API — see Phase 4. Each leaked
      node holds an arena id that is never recycled
      (`lazily-js/src/reactive.js:428-460`).
- [ ] **3d. Add negative assertions throughout.** Every current test is
      one-sided. Needed: nothing recomputed on unrelated update
      (`lite.test.js:112-115` checks `recomputes === r0 + 1` but never that an
      unrelated update leaves it at `r0`), no leak, no re-mount, cleanup fired,
      errors thrown.
- [ ] **3e. Fix test isolation.** `DEFAULT_CTX` (`owner.js:23`) is a module
      global shared across every test in a file and never reset —
      `resetReactiveContext` is exported and never called. Tests are
      order-coupled within a file.

**Untested surface** (grep count = 0 across `test/`): `useReducer`,
`useCallback`, `useId`, `useLayoutEffect`, `useDebugValue`, `forwardRef`, `memo`,
`cloneElement`, `isValidElement`, `raw`, `fragment`, `tagsNS`, `attach`,
`mergeProps`, `onCleanup`, `untrack`, `renderToStaticMarkup`, SVG rendering (the
40-tag `SVG_TAGS` set at `lite.js:25-33`), and `SyncedView.applyDelta`.
`src/full/reconcile.js` (78 lines, the vendored udomdiff LIS — the headline
feature justifying `/full`'s 6.41 kB) and `src/full/constants.js` (610 lines) have
**zero coverage**; `test/full.test.js` is a near-verbatim copy of
`test/lite.test.js` with no list test at all.

- [ ] **3f. Add a real-browser test run.** No Playwright/puppeteer, no
      `test:browser` script. Everything is validated against linkedom, which
      diverges exactly on this library's hot paths: `decideAttr`
      (`lite.js:49-64`) walks a prototype chain that **is not the browser's**
      and caches the result globally; event delegation is never exercised
      (`customElements` is stubbed at `lite.test.js:15`); no CSSOM, layout, SVG
      namespace semantics, focus/selection preservation, or `template.content`
      parsing. The tests already document one divergence in a comment
      (`test/hydrate.test.js:88-90`).

---

## Phase 4 — Version range (`#lzhtml-ver`)

**Correction to an earlier read:** the current declaration is internally
consistent, not drifted. `package.json:32,35` declares `^0.23.0`;
`package-lock.json:718-721` resolves 0.23.0 **from the registry** (not the local
sibling); the code uses **no** 0.24-only API. Tests exercise the declared
version.

The real problems are the inverse:
- A consumer already on lazily-js 0.24.0 gets an unsatisfiable peer warning (or
  a hard install failure under strict pnpm) despite full compatibility.
- The Phase 3c leak fix **requires** 0.24: `disposeCell`/`disposeSlot` are the
  *entire* public-surface delta between 0.23 and 0.24 (the rest is the internal
  `#lzjsarenanodes` typed-array refactor).

- [ ] Bump peer range to `>=0.23.0 <0.25.0`, then to `^0.24.0` once Phase 3c lands.

---

## Phase 5 — Honest framing (`#lzhtml-framing`)

The README presents the accessor return as *the one* semantic deviation
(`README.md:42-46`). There are at least six independent breaks, four of which
produce wrong output with no diagnostic. The gap is understated by roughly an
order of magnitude.

- [ ] **5a. Lead with "renderer", not "Preact/React-compatible API"** in
      `package.json:4` and the README. The current phrasing reads as a
      *binding*, which is what lazily-react is. Honest framing: *"A
      Preact-compatible renderer that replaces React — same API surface, no
      VDOM, SSR + resumable hydration."*
- [ ] **5b. Document all semantic deviations**, not just the accessor:
      functional `setState` unsupported; deps arrays accepted-and-ignored;
      props frozen at mount; no error boundaries (throws swallowed to
      `console.error` at `mount.js:171`, `component.js:157`).
- [ ] **5c. Decide: remove or implement `memo` and `forwardRef`.** Both are
      currently **inert** — `context.js:95-104` sets `_isMemo`/`_equal`,
      `:83` sets `_isForwardRef`, and grep across `mount.js`/`full/index.js`/
      `server.js` finds **no readers**. Exporting inert versions invites
      assumptions that cannot be verified. Recommend removing until implemented.
- [ ] **5d. State plainly that this is not drop-in for Preact.** Aliasing
      `preact → lazily-html` fails at build time, before any semantics matter:
      no `preact/jsx-runtime` (modern JSX transform breaks outright), no
      `preact/hooks`, no `preact/compat`. Also missing: `createRef`,
      `toChildArray`, `Children`, `createPortal`, `Suspense`, `lazy`,
      `StrictMode`, `PureComponent`, `useImperativeHandle`, `options`.
      `render`/`hydrate` signatures differ from Preact's.
- [ ] **5e. `/full` is not API-identical to lite** (contra `README.md:18-19`,
      `full/index.js:7`). `/full`'s `h` is a hyperscript returning a `$ELEMENT`
      thunk, so `isValidElement(h(...))` is false and outputs aren't
      interchangeable. `full/index.js:27` imports `h as hLite` and never uses it.
- [ ] **5f. Do not advertise `/full` as glitch-free** until `untrack` is real
      (Phase 7).
- [ ] **5g. Fix the TypeScript surface.**
      - `index.d.ts:19` declares `render(): () => void`; it returns a `Node`
        (`lite.js:231`). TS users get a runtime `TypeError`.
      - `full/index.d.ts:5` `export * from "../index.d.ts"` — a `.d.ts`
        extension in a module specifier is invalid under every resolution mode.
        `/full`'s types don't resolve at all. It also re-declares `h` (line 7)
        while star-exporting `h` → duplicate-export conflict once fixed.
      - `Setter<T>` (`index.d.ts:5`) accepts `(prev: T) => T`, **documenting a
        bug** — the functional-update form is broken.
      - Undeclared but exported: `mount`, `isClassComponent`, `withInstance`,
        `getCurrentInstance`, `reactiveMemo`, `getReactiveContext`,
        `setReactiveContext`. `effect` declared `: void` but returns a handle
        (`owner.js:121`); `source` omits its `handle` field (`owner.js:142`).

---

## Phase 6 — Sync with a lazily server (`#lzhtml-sync`)

**Current claim status: aspirational.** lazily-html contains **zero network
code** — no WebSocket, fetch, EventSource, transport interface, or bespoke
protocol. Nothing from `distributed`, `relay`, `coordination`, `membership`,
`presence`, `indexeddb-outbox`, or the `#lzsync` reliable-sync plane is imported
anywhere. What exists is a static `{version, anchors}` object handed from
`renderToStreamable` to `SyncedView.applySnapshot` **in the same process**;
`README.md:164-166` wires updates with a manual `addEventListener`.

`SyncedView.applyDelta` is a hard `throw` (`src/synced-view.js:39-41`).

### Why the current wire format can't be upgraded incrementally

Not one field overlaps with real lazily sync:

| lazily-html | real `GraphView` |
|---|---|
| `{version:1, anchors:{a1:"count: 0"}}` (`server.js:292`) | `snapshot.nodes[]` + `edges[]` + `epoch` (`lazily-js/src/graph-view.js:55-66`) |
| ordinal string ids `a1`,`a2` from a counter (`server.js:47-48`) | numeric `NodeId` + `typeTag` |
| rendered display strings | `NodeStatePayload`/`IpcValueInline` raw bytes |
| — | typed `DeltaOpNodeAdd/CellSet/SlotValue/Invalidate/NodeRemove/EdgeAdd/EdgeRemove` (`graph-view.js:72-91`) |

A real lazily frame **fails open, not loudly**: the fallback
`snapshot.anchors || snapshot` (`synced-view.js:25`) would iterate the top-level
keys and create cells named `"nodes"`, `"edges"`, `"epoch"`.

### Work

- [ ] **6a. Export `GraphView` from lazily-js** — `src/graph-view.js` is not in
      `index.js`/`index.d.ts` and has **no `./graph-view` package subpath** (33
      subpaths exist, including `./distributed`, `./relay`,
      `./indexeddb-outbox`, but not this one). **Blocks everything below.**
- [ ] **6b. NodeId ↔ anchor-id mapping.** SSR must emit the server's real
      `NodeId` in `data-lzh`, not a render ordinal. Today ids are minted in DOM
      walk order, so any conditional branch, list-length change, or reorder
      renumbers every downstream anchor — binding is purely **positional** with
      no mismatch detection and no dev warning. Symptomatically,
      `test/hydrate.test.js:113` searches the snapshot **by value** rather than
      pinning an id. Changes `server.js:46-52` and the `data-lzh` grammar.
      **One of the two genuinely hard design decisions.**
- [ ] **6c. Payload codec.** `GraphView` nodes carry opaque `IpcValueInline`
      bytes (`graph-view.js:25-30`); needs a `typeTag` convention for
      "renderable scalar" plus bytes↔string.
- [ ] **6d. Replace `SyncedView` internals** with a real `GraphView` + projection
      layer; implement `applyDelta` over the seven `DeltaOp*` classes.
- [ ] **6e. Wire a `SyncDriver`** — `{sink, source, outbox, clock, provider,
      lastEpoch}` with `tick()`, `enqueue(epoch, msg)`, `onReconnect()`
      (`lazily-js/src/index.js:1480-1526`). WebSocket sink/source,
      `IndexedDbStore`-backed `Outbox` (`indexeddb-outbox.js:8-60`,
      `index.js:1519-1546`), `ResyncCoordinator` epoch tracking + gap detection
      + `RequestSnapshot` (`index.js:1222-1270`), host tick loop.
      lazily-html currently tracks **no epoch at all**, so it cannot detect a
      gap even in principle.
- [ ] **6f. Backpressure** via `RelayCell` + `BackpressurePolicy` + `Overflow`
      (`lazily-js/src/relay.js:62,78`) on the inbound delta path, so a fast
      server can't unbounded-queue in a slow tab. `SyncedView.set` currently
      writes straight through with no bound, drop policy, or spill.
- [ ] **6g. Event wiring — the other hard problem.** Server-rendered pages are
      **inert**: `server.js:108` drops every `on*` prop; `hydrate.js:45-59` binds
      only text and attributes, never re-attaches listeners, never delegates,
      never replays pre-hydration events. Either serialize `on*` to a
      hyop-style registry the client resolves, or ship real dom-expressions SSR
      (`data-hk` + `_$HY`) so `full/client.js:252` works. **Without this, 6a-6f
      yields a live-updating page nobody can interact with.**
      Note `/full`'s hydration is currently dead code: `sharedConfig` is an
      all-null stub (`owner.js:166-178`) and `renderToString` emits no `data-hk`,
      so calling it guarantees the mismatch path at `full/client.js:276-281`.
- [ ] **6h. `applySnapshot` must clear before applying.** `synced-view.js:24-34`
      is additive-only — keys present in the view but absent from a new snapshot
      keep stale values. Real `GraphView.applySnapshot` clears first
      (`graph-view.js:56-57`) precisely because a snapshot is full-state
      replacement. So the one recovery primitive a sync client needs — *"adopt
      the server's fresh snapshot"* — is broken even for the bespoke format.
- [ ] **6i. Add a dev-mode diagnostic for unanchored reactive expressions.**
      `renderToStreamable` anchors only sole-function `children` and
      function-valued non-`on` props (`server.js:150-172`). Explicitly excluded:
      `style` and `classList` (`server.js:157-159`). Array children never anchor
      — a function inside an array is called and baked in
      (`server.js:243-250`). So `h("div", {style: () => ({color: c()})})` and
      `h("ul", null, [() => a(), () => b()])` render correct initial HTML,
      produce no anchor, and are permanently dead DOM after hydrate with no
      warning. Documented at `server.js:277-278`, but it will be discovered in
      production without a diagnostic.
- [ ] **6j. Restore the global reactive context.** `hydrate.js:19` calls
      `setReactiveContext(view.ctx)` (`owner.js:31-33` assigns the module-level
      `DEFAULT_CTX`) and **never restores it** — compare `renderToStreamable`,
      which correctly saves/restores `ANCHOR_STATE` (`server.js:286,295`). After
      any `hydrate()`, every subsequent `render()`/`root()` binds into that
      `SyncedView`'s context. Two islands on one page: the second wins globally.
      `SyncedView.activate()` (`synced-view.js:63-66`) has the same defect.
      Also relevant to concurrent SSR.
- [ ] **6k. Add sync tests.** `test/hydrate.test.js:28-127` is six single-process
      tests where `view.set` is a *local mutation*, not an inbound frame. No
      test has a second peer, an epoch, a delta, a disconnect, or an encoded
      frame.

**Until 6a-6g land, the README's sync section (`README.md:139-176`) should say
what the code does:** *SSR emits a value map; the client applies it to local
cells, in-process.*

---

## Phase 7 — Upstream lazily-js asks (`#lzhtml-upstream`)

- [ ] **7a. Export `GraphView`** (= 6a).
- [ ] **7b. Expose an `untrack` / tracking-stack toggle** (`reactive.js:189`).
      lazily-js has **no `untrack`** (`grep -rn untrack lazily-js/src` → zero
      hits), so `owner.js:162-164` is a genuine capability gap, not a shim. Its
      justification — that component bodies never run inside a tracking compute
      (`owner.js:154-160`) — holds for `lite` but is **false for `/full`**:
      `full/client.js:508,518` create effects whose bodies reach
      `createComponentFull` → the component body, so any state read there
      subscribes the enclosing insert effect and re-creates the whole subtree on
      every change. `full/client.js:207` likewise runs ref callbacks inside
      `effect(...)`, contradicting the comment's "ref callbacks fire outside any
      compute". `owner.js:211-213` documents `createComponent` as calling the
      component untracked, and it does not.

---

## Phase 8 — Positioning vs lazily-react (`#lzhtml-position`)

Both packages are `@lazily-hub/*`, both at `0.1.0`, both taglines contain
"React/Preact", and **neither mentions the other** (grep: zero cross-references;
`README.md:11-14` names `lazily-slint`/`lazily-tui`, not lazily-react).

They are architecturally opposite — *bring lazily to React* (additive) vs
*replace React with lazily* (a migration):

| | lazily-react | lazily-html |
|---|---|---|
| What | state binding into *someone else's* React | renderer that replaces React |
| How | `useSyncExternalStore` (`hooks.js:101`) | own fine-grained DOM runtime |
| Size | 3 files, ~330 lines, 8 exports | 5,490 lines, two renderers |
| SSR | none | core feature |
| React dep | peer, **optional** (`package.json:40-42`) | none — it *is* the renderer |

**The concrete footgun:** incompatible `useState`-shaped hooks. lazily-html's
`useState` returns an accessor `[() => T, set]` (`hooks.js:30-37`);
lazily-react's `useCell` returns a value `[T, set]` (`hooks.js:150`). Install
both and you render `[object Function]`.

- [ ] **8a.** Add a mutual "which one do I want?" block to both READMEs. Decision
      rule: *already on React and staying → lazily-react; building new, or
      willing to swap the renderer → lazily-html.* Cheapest high-value fix here.
- [ ] **8b.** Add lazily-react to lazily-html's sibling list under a separate
      **integrations** heading — it isn't a render target, so it doesn't belong
      in the `lazily-slint`/`lazily-tui` series.
- [ ] **8c.** Document lazily-html's accessor deviation in lazily-react too, as a
      "this is NOT how the other package works" note. Call it out from both sides.
- [ ] **8d. DECISION — rename `lazily-react`?** It's a framework-agnostic
      subscription bridge whose React dep is already optional, and its own
      AGENTS.md calls `src/bridge.js` "the core IP". `lazily-bind` + a thin
      `lazily-react` wrapper would kill the ambiguity permanently. **Cheap now
      at 0.1.0 and unpublished; expensive later.** Needs an operator call.

---

## Phase 9 — Reuse what lazily-js already ships (`#lzhtml-reuse`)

- [ ] **9a. Keyed collections are hand-rolled.** `src/full/reconcile.js` vendors
      udomdiff LIS and `lite` has no keyed path at all, while lazily-js exports
      `reconcileCollections` (LIS + stable-key preservation,
      `collections.js:233`) and the `ReactiveMap`/`CellMap`/`SlotMap` family
      (`reactive-family.js:57,338,400`) with per-key entry handles and separate
      membership/order signals. Neither is imported.
      **Nuance: these solve different layers** (DOM node moves vs keyed reactive
      entries), so `reconcile.js` is not strictly redundant — the gap is the
      **missing per-key owner layer between them**, which is also the natural
      home for per-item disposal (Phase 1). That family is the right substrate
      for a `<For>` primitive.
- [ ] **9b. `key` is parsed and then ignored.** `create-element.js:17` stores it;
      `mount.js` never reads `vnode.key`; `lite.js:67` strips it.
      `insertChild`'s effect (`lite.js:178-189`) replaces the whole node set
      wholesale, so list children lose local state and leak owners on any array
      change.

---

## Dead code to remove

- [ ] `removeMany` — `src/lite.js:213-216`, unused.
- [ ] `isMemoComponent` (`context.js:44`) / `isForwardRef` — no callers.
- [ ] `component.js:149` `if (inst.state == null) inst.state = {}` — dead; the
      `state` getter (`:31`) always materializes a proxy.
- [ ] `full/index.js:27` `import { h as hLite }` — never used.
- [ ] `markAccessor` (`owner.js:216-219`) — currently unused, but **do not
      delete**: it is the missing half of the 2c fix.

## Latent / watch

- `hooks.js:63` returns before allocating a hook slot on the server, so hook
  indices differ between server and browser for the same component. Masked today
  because hydration doesn't re-execute bodies; becomes corruption the moment it
  does (or if 6g re-runs bodies).
- `hooks.js:70-76` runs the previous cleanup *inside* the tracked compute, so
  cleanup reads accidentally register dependency edges.
- `lite.js:97-102`: a null `on*` prop is a no-op, so a handler can be added but
  never removed. Only reachable via spread-style updates, which lite lacks.
- The whole `deps` machinery is dead (`hooks.js:61-68`) because bodies run once.
  Combined with the missing `untrack`, a user **cannot express React's
  `[]`-means-once semantics at all**. Accepting-and-ignoring the parameter is
  worse than rejecting it — consider throwing in dev mode.

---

## Suggested execution order

1. **Phase 0** — unblocks everything, no code risk.
2. **Phase 1** (owner tree) + **2a/2b/2c** — the root bug plus the three cheap
   visible-wrongness fixes. Biggest correctness win per line changed.
3. **Phase 3** (disposer + real tests) — locks in Phase 1 against regression.
4. **Phase 5** (framing) — cheap, and stops the claims from aging worse.
5. **Phase 4** + remaining Phase 2.
6. **Phase 8** positioning (8a-8c cheap; 8d needs an operator decision).
7. **Phase 6/7** (sync) — the real project. 6b and 6g are the genuinely hard
   design work; 6a and 6c-6f are integration.

## Open questions for the operator

- **8d:** rename `lazily-react` → `lazily-bind` while both are 0.1.0 and
  unpublished?
- **5c:** remove inert `memo`/`forwardRef`, or implement them?
- **Phase 6 scope:** is real lazily-server sync the near-term goal, or should
  the README be de-scoped to in-process hydration and sync deferred?
- Does the `deps` parameter stay (accepted-and-ignored), throw in dev, or get
  removed from the signature?
