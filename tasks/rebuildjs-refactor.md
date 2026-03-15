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

### Phase 1: SPEC.md
- [ ] Write `rebuildjs/SPEC.md` — adapter interface contract, subpath exports, framework adapter requirements
- [ ] Define TypeScript interfaces for framework adapters

### Phase 2: Extract shared logic into rebuildjs
- [ ] Move route logic (html_route_, request_ctx__ensure, html_response__new) — parameterize by framework context type
- [ ] Move compression middleware — parameterize by middleware return type
- [ ] Move static file serving
- [ ] Move SSG export + Cloudflare export
- [ ] Create `rebuildjs/server/elysia` adapter subpath
- [ ] Create `rebuildjs/server/hono` adapter subpath
- [ ] Remove `elysia` from rebuildjs direct deps
- [ ] Add new subpath exports to rebuildjs package.json

### Phase 3: Thin wrappers
- [ ] Update relysjs to re-export from rebuildjs
- [ ] Update rhonojs to re-export from rebuildjs
- [ ] Verify existing import paths still work (relysjs/server, rhonojs/server)
- [ ] Move elysia dep to relysjs only, hono dep to rhonojs only

### Phase 4: Tests
- [ ] Port relysjs tests to rhonojs
- [ ] Add adapter interface conformance tests
- [ ] Run all existing tests to verify no regressions

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
Phase 3+4 launched (thin wrappers + tests). Agent will:
1. Verify relysjs/rhonojs re-export from rebuildjs correctly
2. Confirm elysia dep only in relysjs, hono dep only in rhonojs
3. Port relysjs tests to rhonojs
4. Add adapter conformance tests
5. Run all tests

Session is free.

Please update Phase progress
<!-- /agent:exchange -->
