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
Got it — briantakita.me runs on Cloudflare Pages, not Docker/Contabo. The code is already committed and pushed to main (`41fbe49`). If Cloudflare Pages is connected to the repo, it should auto-deploy.

If you need a manual deploy: run `bun prerender.ts` with the server running, then push the static output to Cloudflare Pages via `wrangler pages deploy dist/static` (or however your CF Pages project is configured).

The Makefile with Docker targets is stale — can be cleaned up separately.

**Refactor complete.** Everything is committed, pushed, and published.

briantakita.me does not need docker anymore...since we are exclusively running on Cloudflare...not on Contabo.

Got an error with drmanny-dev

```
$ cd ./app/peakexistence-site && bun dev
$ bun run dev:sourcemap__resolve__bun
$ sourcemap__resolve__bun -f index.ts
relysjs_browser__build|done
relysjs_server__build|done
browser__build|watch
server__build|watch
✘ [ERROR] ENOENT: no such file or directory, open '/home/brian/work/btakita/agent-loop/src/drmanny-dev/app/peakexistence-site/icon/fa_waves_sine.svg' [plugin object-store-asset]

    config.ts:19:30:
      19 │ import fa_waves_sine_svg from './icon/fa_waves_sine.svg.file.js'
         ╵                               ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

  This error came from the "onLoad" callback registered here:

    ../../node_modules/.bun/esbuild-plugin-object-store-asset@0.0.2+0ae84e4a3e13ab91/node_modules/esbuild-plugin-object-store-asset/index.js:14:9:
      14 │       build.onLoad({ filter }, async ({ path })=>{
         ╵             ~~~~~~

    at setup (/home/brian/work/btakita/agent-loop/src/drmanny-dev/node_modules/.bun/esbuild-plugin-object-store-asset@0.0.2+0ae84e4a3e13ab91/node_modules/esbuild-plugin-object-store-asset/index.js:14:10)
    at <anonymous> (/home/brian/work/btakita/agent-loop/src/drmanny-dev/node_modules/.bun/esbuild@0.27.4/node_modules/esbuild/lib/main.js:1422:21)
    at buildOrContextImpl (/home/brian/work/btakita/agent-loop/src/drmanny-dev/node_modules/.bun/esbuild@0.27.4/node_modules/esbuild/lib/main.js:1135:5)
    at buildOrContext (/home/brian/work/btakita/agent-loop/src/drmanny-dev/node_modules/.bun/esbuild@0.27.4/node_modules/esbuild/lib/main.js:961:5)
    at Promise (unknown:1:11)
    at context (/home/brian/work/btakita/agent-loop/src/drmanny-dev/node_modules/.bun/esbuild@0.27.4/node_modules/esbuild/lib/main.js:2325:31)
    at rebuildjs_server__build (/home/brian/work/btakita/agent-loop/src/drmanny-dev/node_modules/.bun/rebuildjs@0.70.45+921ee3b03b0a9bbc/node_modules/rebuildjs/server/build/index.js:345:28)
    at processTicksAndRejections (unknown:7:39)

✘ [ERROR] JSON Parse error: Unexpected EOF [plugin onEnd]

    /home/brian/work/btakita/agent-loop/src/drmanny-dev/node_modules/.bun/esbuild@0.27.4/node_modules/esbuild/lib/main.js:1202:47:
      1202 │ ...sponse.metafile) result.metafile = parseJSON(response.metafile);
           ╵                                       ^

    at <parse> (:0)
    at parse (unknown)
    at buildResponseToResult (/home/brian/work/btakita/agent-loop/src/drmanny-dev/node_modules/.bun/esbuild@0.27.4/node_modules/esbuild/lib/main.js:1202:48)
    at <anonymous> (/home/brian/work/btakita/agent-loop/src/drmanny-dev/node_modules/.bun/esbuild@0.27.4/node_modules/esbuild/lib/main.js:1217:9)
    at new Promise (native:1:11)
    at <anonymous> (/home/brian/work/btakita/agent-loop/src/drmanny-dev/node_modules/.bun/esbuild@0.27.4/node_modules/esbuild/lib/main.js:1216:58)
    at <anonymous> (/home/brian/work/btakita/agent-loop/src/drmanny-dev/node_modules/.bun/esbuild@0.27.4/node_modules/esbuild/lib/main.js:909:17)
    at handleIncomingPacket (/home/brian/work/btakita/agent-loop/src/drmanny-dev/node_modules/.bun/esbuild@0.27.4/node_modules/esbuild/lib/main.js:934:7)
    at readFromStdout (/home/brian/work/btakita/agent-loop/src/drmanny-dev/node_modules/.bun/esbuild@0.27.4/node_modules/esbuild/lib/main.js:862:7)
    at emit (node:events:95:22)
    at addChunk (internal:streams/readable:265:47)
```
<!-- /agent:exchange -->
