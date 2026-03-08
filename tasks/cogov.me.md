---
agent_doc_session: 159f6a0e-f91f-4485-87e2-7e85474bb00d
tmux_session: '0'
agent_doc_mode: template
---

# cogov.me

<!-- agent:status -->
**Status:** rhonojs migration complete. Ready to commit across repos.
<!-- /agent:status -->

<!-- agent:exchange -->

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
- `herbaliciousbliss-dev/` — 55 files migrated from relysjs/Elysia to rhonojs/Hono

## Pending

- [x] herbaliciousbliss → rhonojs migration (55 files across 10 submodules)
- [ ] Commit all uncommitted work across repos
- [ ] Publish rhonojs to npm
- [ ] Deploy demo.cogov.me to CF Workers

- Converted to `agent:exchange`. The input/output split is replaced by a single exchange component where both user and agent write inline.

Also updated pending checklist — herbaliciousbliss migration marked done, added herbaliciousbliss to uncommitted work.

Release rhonojs on npm. Do whatever commits + pushes we need to do. Skip agent-doc b/c another session is working on it. 
<!-- /agent:exchange -->
