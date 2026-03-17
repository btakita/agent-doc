---
agent_doc_session: fce94c2a-2072-49c6-924b-144e96b540b7
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---


The @rappstack/* packages were recently published.
The apps used submodules from rappstack.
Now that @rappstack/* packages are published, the apps need to reference the @rappstack/* packages instead of using submodules.

Convert brookebrodack-dev, cogov-dev, censible-core, drmanny-dev, herbaliciousbliss-dev, prescience-notes-dev

Use yalc if possible. And deploy via github actions. The deployments must depend on the published npm packages...not yalc. yalc is only for development.

<!-- agent:exchange -->

### Compacted Exchange (prior conversation summary)

**drmanny-dev assets:** Fixed by re-adding `esbuild-plugin-object-store-asset` + uploading 122 assets to R2 with SHA256-hashed filenames. CI passed.

**bun build vs esbuild:** Bun's bundler has `onResolve`/`onLoad` hooks but lacks `onStart`/`onEnd`/`onDispose`, `build.initialOptions`, and per-build plugin registration. Most rappstack plugins would port (`object_store_asset`, `esmcss`, `preprocess`, `esmfile`), but `rebuild_tailwind_plugin_` is questionable. The bigger blocker is `rebuildjs`/`relysjs`/`rhonojs` — deeply coupled to esbuild's build/watch lifecycle. No dedicated bun build Tailwind plugin exists; would need a custom `onLoad` hook running `@tailwindcss/postcss`.

**Watch mode:** esbuild gives sub-50ms incremental rebuilds via `ctx.rebuild()` + `onEnd` callbacks. Bun's `--watch` restarts the entire process (no incremental, no `onEnd` equivalent). The `rebuildjs` lifecycle hooks have no direct bun equivalent.

**Key architecture decision — rebuildjs as core with thin wrappers:**

```
rebuildjs (core)
├── rebuildjs/elysia   — optional elysia adapter
├── rebuildjs/hono     — optional hono adapter
├── rebuildjs/server   — shared server logic (static files, prerender, ready__wait)
└── rebuildjs/build    — esbuild orchestration

relysjs (thin wrapper) → dep: elysia, re-exports rebuildjs/elysia
rhonojs (thin wrapper) → dep: hono, re-exports rebuildjs/hono
```

- Apps keep existing import paths (`relysjs/server`, `rhonojs/server`) — no change needed
- Single place to fix build bugs
- Future bun build support goes in `rebuildjs/build` with an adapter

**Implementation plan created:** `tasks/rebuildjs-refactor.md` with 4 phases:
1. SPEC.md for rebuildjs (bundler-agnostic interface + adapter contract)
2. Extract shared logic into rebuildjs subpaths
3. Make relysjs/rhonojs thin re-export wrappers
4. Tests for rhonojs + integration tests verifying both adapters
<!-- /agent:exchange -->
