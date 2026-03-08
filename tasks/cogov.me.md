---
agent_doc_session: 159f6a0e-f91f-4485-87e2-7e85474bb00d
tmux_session: '0'
agent_doc_mode: template
---

# cogov.me

<!-- agent:status -->
**Completed:** herbaliciousbliss → rhonojs migration (55 files across 10 submodules)
<!-- /agent:status -->

<!-- agent:input -->
<!-- /agent:input -->

<!-- agent:output -->
### herbaliciousbliss → rhonojs migration — DONE

55 files changed across 10 submodules in herbaliciousbliss-dev:

- **31 files** — type-only imports changed to `rebuildjs/server`
- **12 files** — Hono-specific imports changed to `rhonojs/server`
- **3 files** — Elysia framework code → Hono (`new Elysia()` → `new Hono()`, context patterns)
- **2 files** — Context type updates (Elysia context → Hono Context)
- **1 file** — Build system (`relysjs_browser__build` → `rhonojs_browser__build`)
- **10 package.json** — dependency updates (relysjs → rhonojs, elysia → hono)

Zero remaining `relysjs` imports in source files (verified).

**Next:** Commit all uncommitted work across repos, then publish rhonojs to npm.
<!-- /agent:output -->

## Session History (compacted)

### cogov.me static site fixes
- **protocol.love download bug** — fixed `static_export__file_path_()` in relysjs to use SSR Content-Type header instead of guessing from file extension
- **CSS missing after deploy** — fixed by using production build flow (`NODE_ENV=production`) so CSS/JS/HTML all go to `dist/browser/`
- **Tests added** — 11 tests for `static_export__file_path_()` in relysjs
- **Branding** — updated cogov.tech references to cogov.me
- **README** — replaced lerna/yarn with bun

### demo.cogov.me (Hono + D1 + CF Workers)
- **Scaffolded** — Hono Workers app with D1, Drizzle ORM, 7 views (all CoGov.Vue views ported)
- **Routes** — person, collectives, council, members, privilege-set, proposal
- **Stack** — Hono, D1, Drizzle, relementjs, CF Workers
- **Wired to rhonojs** — dependency added via yalc

### rhonojs package (ctx-core-dev/lib/rhonojs)
- **Created** — 6 server modules ported from relysjs (Elysia → Hono)
- **Modules** — server/hono, server/app, server/build, server/route, server/compression, server/static, server/export, server/export/cloudflare
- **Dual support** — both relysjs and rhonojs coexist; rappstack works with either via rebuildjs

### rappstack-dev dual support
- **28 files** changed `from 'relysjs/server'` → `from 'rebuildjs/server'`
- **6 files** still import from relysjs (Elysia-specific: `request_`, `request_url_`, `request_ctx__ensure`)
- **Key insight** — rebuildjs/server is the shared foundation; both relysjs and rhonojs re-export from it

## Uncommitted Work

- `ctx-core-dev/lib/rhonojs/` — entire new package (20+ files)
- `cogov-dev/app/demo.cogov.me/` — rhonojs dependency + cookie fixes + new routes
- `rappstack-dev/lib/*/` — 28 import changes across 6 packages

## Pending

- [ ] herbaliciousbliss → rhonojs migration (43 files, 7 app + 36 lib)
- [ ] Commit all uncommitted work across repos
- [ ] Publish rhonojs to npm
- [ ] Deploy demo.cogov.me to CF Workers

Convert this doc to use agent:exchange instead of agent:input/agent:output.
