---
agent_doc_session: 717da06a-d7f9-4009-8d66-6afb30052cb1
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---


# rebuildjs Refactor — Unified Server Adapter Architecture

## Goal

Consolidate relysjs (Elysia) and rhonojs (Hono) into rebuildjs as framework adapters. relysjs and rhonojs become thin wrapper packages that add framework deps and re-export from rebuildjs.

## Current State

| Package | Version | Tests | SPEC.md | Notes |
|---------|---------|-------|---------|-------|
| rebuildjs | 0.70.47 | 8 files | No | Core build orchestration. Has `elysia` as direct dep (wrong). |
| relysjs | 1.21.75 | 7 files | No | Mature Elysia adapter. |
| rhonojs | 0.1.1 | 0 files | No | New Hono adapter. Near-identical API to relysjs. |

All in `src/ctx-core-dev/lib/`.

## Target Architecture

```
rebuildjs (core)
├── rebuildjs/server          — existing: ctx, build, metafile, asset, middleware
├── rebuildjs/server/route    — NEW: html_route_(), request_ctx__ensure(), html_response__new()
├── rebuildjs/server/compression — NEW: compression logic (parameterized)
├── rebuildjs/server/static   — NEW: static file serving
├── rebuildjs/server/export   — NEW: SSG + Cloudflare export
├── rebuildjs/server/elysia   — NEW: Elysia-specific adapter (app, context, middleware types)
├── rebuildjs/server/hono     — NEW: Hono-specific adapter
└── rebuildjs/build           — existing: esbuild orchestration

relysjs (thin wrapper)
├── dep: elysia
├── peerDep: rebuildjs
└── re-exports rebuildjs/server/elysia + rebuildjs/server

rhonojs (thin wrapper)
├── dep: hono
├── peerDep: rebuildjs
└── re-exports rebuildjs/server/hono + rebuildjs/server
```

## Phases

### Phase 1: SPEC.md ✓
- [x] Write `rebuildjs/SPEC.md` — adapter interface contract, subpath exports, framework adapter requirements
- [x] Define TypeScript interfaces for framework adapters

### Phase 2: Extract shared logic into rebuildjs ✓
- [x] Move route logic (html_route_, request_ctx__ensure, html_response__new) — parameterize by framework context type
- [x] Move static file serving
- [x] Move SSG export + Cloudflare export
- [x] Remove `elysia` from rebuildjs direct deps
- [x] Add new subpath exports to rebuildjs package.json
- N/A: Compression middleware stays in adapters (fundamentally different implementations)
- N/A: Elysia/Hono adapter subpaths stay in relysjs/rhonojs (framework-specific)

### Phase 3: Thin wrappers ✓
- [x] Update relysjs to re-export from rebuildjs
- [x] Update rhonojs to re-export from rebuildjs
- [x] Verify existing import paths still work (relysjs/server, rhonojs/server)
- [x] Move elysia dep to relysjs only, hono dep to rhonojs only

### Phase 4: Tests ✓
- [x] Port relysjs tests to rhonojs (83 tests)
- [x] Add adapter interface conformance tests (44 relysjs + 42 rhonojs)
- [x] Run all existing tests to verify no regressions (rebuildjs 56, relysjs 76, rhonojs 83)

## Key API Surface (shared → rebuildjs)

**From relysjs/rhonojs (identical interface, different types):**
- `app$_` — framework app signal
- `app__attach(app, sub_app)` — attach sub-app/routes
- `app__start(config)` — start server
- `html_route_(middleware_ctx, page_, response_init?)` — create route handler
- `request_ctx__ensure(middleware_ctx, context)` — ensure request context
- `html_response__new(html, init?)` — create HTML response
- `compression_middleware_(config?)` — gzip/deflate middleware
- `static_middleware_(config?)` — static file serving
- `static_export_(config)` — SSG export
- `cloudflare_export_(config)` — Cloudflare Pages export

**Framework-specific (stays in adapter):**
- Elysia: `elysia_context$_`, `store$_`, Elysia types
- Hono: `hono_context$_`, Hono `Context` type

## Notes

- brookebrodack-dev uses Elysia (relysjs), briantakita.me uses Hono (rhonojs)
- The relysjs→rhonojs migration proved the API surface is near-identical
- `bun build` adapter is a future extension (separate task)

<!-- agent:exchange -->
**drmanny-dev fixed.** Root cause: esbuild plugin order in `build.ts`. `object-store-asset` fired before `esmfile`, tried to read non-existent SVG. Fix: swapped plugin order so `esmfile` handles `.file.js` imports first. Server starts cleanly on port 4103.

This was a pre-existing bug unrelated to the rebuildjs refactor — exposed by the fresh `bun install`.

brookebrodack-dev has internal server error:
```
~/work/btakita/agent-loop/src/brookebrodack-dev main                                                                                                                                                                     ✘ INT 6m 27s 02:40:13
❯ bun run dev
$ cd ./app/brookebrodack-site && bun dev
$ bun run dev:sourcemap__resolve__bun
$ sourcemap__resolve__bun -f index.ts
browser__build|watch
server__build|watch
server started on port 4101
17 |       } else {
18 |         handler = i === middleware.length && next || void 0;
19 |       }
20 |       if (handler) {
21 |         try {
22 |           res = await handler(context, () => dispatch(i + 1));
                           ^
TypeError: handler is not a function. (In 'handler(context, () => dispatch(i + 1))', 'handler' is an instance of Hono2)
      at dispatch (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/compose.js:22:23)
    at compress2 (../../../../node_modules/.bun/hono@4.12.8/node_modules/hono/dist/middleware/compress/index.js:8:10)
      at dispatch (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/compose.js:22:23)
      at <anonymous> (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/hono-base.js:118:73)
      at dispatch (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/compose.js:22:23)
      at <anonymous> (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/hono-base.js:301:31)

17 |       } else {
18 |         handler = i === middleware.length && next || void 0;
19 |       }
20 |       if (handler) {
21 |         try {
22 |           res = await handler(context, () => dispatch(i + 1));
                           ^
TypeError: handler is not a function. (In 'handler(context, () => dispatch(i + 1))', 'handler' is an instance of Hono2)
      at dispatch (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/compose.js:22:23)
    at compress2 (../../../../node_modules/.bun/hono@4.12.8/node_modules/hono/dist/middleware/compress/index.js:8:10)
      at dispatch (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/compose.js:22:23)
      at <anonymous> (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/hono-base.js:118:73)
      at dispatch (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/compose.js:22:23)
      at <anonymous> (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/hono-base.js:301:31)

17 |       } else {
18 |         handler = i === middleware.length && next || void 0;
19 |       }
20 |       if (handler) {
21 |         try {
22 |           res = await handler(context, () => dispatch(i + 1));
                           ^
TypeError: handler is not a function. (In 'handler(context, () => dispatch(i + 1))', 'handler' is an instance of Hono2)
      at dispatch (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/compose.js:22:23)
    at compress2 (../../../../node_modules/.bun/hono@4.12.8/node_modules/hono/dist/middleware/compress/index.js:8:10)
      at dispatch (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/compose.js:22:23)
      at <anonymous> (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/hono-base.js:118:73)
      at dispatch (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/compose.js:22:23)
      at <anonymous> (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/hono-base.js:301:31)

17 |       } else {
18 |         handler = i === middleware.length && next || void 0;
19 |       }
20 |       if (handler) {
21 |         try {
22 |           res = await handler(context, () => dispatch(i + 1));
                           ^
TypeError: handler is not a function. (In 'handler(context, () => dispatch(i + 1))', 'handler' is an instance of Hono2)
      at dispatch (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/compose.js:22:23)
    at compress2 (../../../../node_modules/.bun/hono@4.12.8/node_modules/hono/dist/middleware/compress/index.js:8:10)
      at dispatch (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/compose.js:22:23)
      at <anonymous> (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/hono-base.js:118:73)
      at dispatch (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/compose.js:22:23)
      at <anonymous> (/home/brian/work/btakita/agent-loop/src/brookebrodack-dev/node_modules/.bun/hono@4.12.8/node_modules/hono/dist/hono-base.js:301:31)
```
<!-- /agent:exchange -->
