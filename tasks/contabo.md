---
agent_doc_session: e42113ab-519f-4c5c-a636-23757ee17337
tmux_session: '0'
agent_doc_mode: template
---

# Contabo → Cloudflare Migration

<!-- agent:status -->
**Status:** 6 of 7 sites migrated. Only censible (mi-www.site) remains on Contabo.
<!-- /agent:status -->

<!-- agent:exchange -->
Compacted from 1437 lines. Template mode active.

### Remaining on Contabo (3 containers)

| Container | Domain | Purpose |
|-----------|--------|---------|
| `mi-www_site-1` | `mi-www.site` | Censible (Solid Start + Postgres + Redis + Auth0) |
| `mi-redis` | — | Redis for Censible |
| `traefik` | — | Reverse proxy (only needed for Censible now) |

### Migrated to Cloudflare (6 sites)

| Site | CF Pages | Custom Domain | Auth | Status |
|------|----------|---------------|------|--------|
| `briantakita.me` | briantakita-me.pages.dev | briantakita.me | N/A | Live |
| `brookebrodack.net` | brookebrodack-net.pages.dev | brookebrodack.net | N/A | Live |
| `pe.briantakita.me` | peak-existence.pages.dev | pe.briantakita.me | N/A | Live |
| `pn.briantakita.me` | prescience-notes.pages.dev | pn.briantakita.me | N/A | Live |
| `hb.briantakita.me` | herbaliciousbliss.pages.dev | hb.briantakita.me | Google OAuth + D1 | Live |
| `cogov.me` | cogov-me.pages.dev | cogov.me | N/A | Live |
<!-- /agent:exchange -->

## Session History (compacted)

### Initial survey
- VPS at 207.244.234.186 running 5 sites + Traefik reverse proxy
- briantakita.me and brookebrodack.net already on CF Pages

### Repo reorganization
- Moved all submodules into `src/` directory (from flat root)
- Layout: `src/` for software projects, `mail/` for email, `tasks/` for sessions

### Migration workflow (per site)
1. Production build locally
2. Deploy to CF Pages via `wrangler pages deploy`
3. Add custom domain CNAME in CF DNS
4. Stop + remove Docker container on Contabo
5. Verify site loads from CF Pages

### Sites migrated (in order)
1. **pe.briantakita.me** (peakexistence) — static site from drmanny-dev
2. **pn.briantakita.me** (prescience-notes) — static site
3. **cogov.me** — static site, DNS A record updated to CNAME
4. **hb.briantakita.me** (herbaliciousbliss) — static deploy (Stage 1), auth worker planned (Stage 2)

### herbaliciousbliss Stage 2 (planned)
- CF Worker + D1 for Google OAuth login
- Drizzle driver swap: bun-sqlite → d1
- Thin worker entry point wrapping bundled server
- Build: `bun build.ts --target worker` → `dist/worker/index.js`
- Source maps for debugging stack traces
- **Note:** rhonojs migration already completed in cogov.me session (55 files, Elysia → Hono)

### Censible decision
- Keeping on Contabo due to different stack (Solid Start, Postgres, Redis, Auth0)
- Could simplify Contabo setup: remove traefik, run censible directly on port 443
