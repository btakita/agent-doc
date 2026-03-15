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
**Doc audit — issues found in this document:**

1. **Current State table (line 17-21):** Shows old versions (rebuildjs 0.70.47, relysjs 1.21.75, rhonojs 0.1.1). Should reflect: rebuildjs 0.71.0, relysjs 1.21.76, rhonojs 0.1.2. SPEC.md exists now. Test counts increased.

2. **Notes (line 95):** Says "brookebrodack-dev uses Elysia (relysjs)" — now uses Hono (rhonojs).

3. **Target Architecture (lines 34-35):** Shows `rebuildjs/server/elysia` and `rebuildjs/server/hono` as "NEW" — these were descoped (stay in adapters).

These are cosmetic — the phase checklists and exchange log tell the accurate story. Want me to update the static sections, or leave them as the "before" snapshot?

**Other docs to check:**
- `src/ctx-core-dev/lib/rebuildjs/SPEC.md` — already written, reflects final state
- Submodule CLAUDE.md files — no changes needed (they describe project structure, not versions)

Did you also update cogov.me?
<!-- /agent:exchange -->
