---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Migration assessment complete. Awaiting decisions on scope (UI rewrite? Auth migration? Deploy target?).
<!-- /agent:status -->

<!-- agent:exchange -->
## Migration Assessment: censible-core → rappstack/rhonojs

### Stack Comparison

| Layer | Current (censible-core) | Target (rappstack/rhonojs) |
|-------|------------------------|---------------------------|
| UI rendering | SolidJS 1.6 (reactive components) | relementjs (functional HTML generation) |
| Server framework | Solid Start 0.2.7 (SSR) | Hono via rhonojs |
| Routing | File-based + itty-router | Hono routes + relementjs page renderers |
| Auth | Auth0 (JWT, OAuth redirect) | Persontric + Arctic (Google OAuth, sessions) |
| Database | Raw SQL + postgresql-client | Drizzle ORM (PostgreSQL adapter available) |
| Caching | Redis (ioredis) | No built-in; can add Redis or KV |
| State mgmt | nanostores + ctx-core context | ctx-core context (same foundation) |
| Build | Vite + Rollup | rebuildjs (esbuild wrapper) |
| Deploy | Heroku (Node 18) | Cloudflare Workers or Bun server |

**Shared foundation:** 26+ @ctx-core/* packages already used in censible-core. This is the bridge.

### Key Architectural Decisions Needed

1. **SolidJS → relementjs**: This is a full UI rewrite. Every component, page layout, and interactive element changes. relementjs uses functional HTML generation (e.g. `div_()`, `span_()`) instead of JSX reactive components. Is this acceptable scope?

YAes

2. **Auth0 → Persontric/Arctic**: Requires user migration strategy. Auth0 manages user credentials externally; Persontric is self-hosted. Options:
   - Keep Auth0 initially, wrap with rhonojs middleware
   - Migrate users to Persontric with password reset flow
   - Support both during transition period

es

3. **PostgreSQL stays**: Drizzle ORM has PostgreSQL adapters. persontric-adapter-postgresql exists for auth tables. Raw SQL can migrate incrementally.

Let's convert to Sqlite on Cloudflare

4. **Redis**: rhonojs doesn't include Redis, but ioredis works fine with Hono. No migration needed for caching layer.

Use cloudflare alternative if necessary

5. **Deployment target**: Cloudflare Workers (via rhonojs/server/export/cloudflare) or stay on traditional server (Bun/Node)?
   - CF Workers: Need D1 or Hyperdrive for PostgreSQL access
   - Bun server: Simpler migration, keep existing PostgreSQL connection

Cloudflare workers

### Recommended Phases

**Phase 1 — Server framework (lowest risk)**
Replace Solid Start + itty-router with rhonojs/Hono for API routes. Keep existing SQL and Redis as-is. This gives the new server foundation without touching UI.
- New entry point using `rhonojs_app__new()`
- Migrate API routes from itty-router to Hono handlers
- Port `entry-server.tsx` request handling to Hono middleware

**Phase 2 — Database layer**
Migrate raw SQL to Drizzle ORM with PostgreSQL adapter.
- Define Drizzle schemas for existing tables
- Replace `query($pg, SQL)` calls with Drizzle queries
- Add persontric tables (person, session) for Phase 3

**Phase 3 — Authentication**
Replace Auth0 with Persontric + Arctic.
- Implement Google OAuth via Arctic (or other providers)
- Session cookie management via Persontric
- User migration: import Auth0 users into person table
- CSRF protection via rappstack patterns

**Phase 4 — UI rendering (largest phase)**
Rewrite SolidJS components to relementjs.
- Server pages: `relementjs/server` HTML generation
- Browser interactivity: `hyop` snippets for progressive enhancement
- Port each app (root-web, esg-web, mi-web) one at a time
- Leverage `@rappstack/ui--server` for layouts, meta tags, RSS, sitemap

**Phase 5 — Build & deploy**
Switch from Vite/Rollup to rebuildjs, choose deployment target.
- `rhonojs_server__build()` + `rhonojs_browser__build()`
- Target: Cloudflare Workers (need Hyperdrive for PostgreSQL) or Bun server
- Decommission Heroku

### Current Architecture (for reference)

**Monorepo:** 9 apps (root-web, esg-web, mi-web, finegar-etl, quiz-web, msci-demo-web, clasp-web, kyc-web, screenshot-web) + 24 shared libs

**Key apps:**
- **root-web** — Marketing/landing site (simpler, good migration candidate)
- **esg-web** — Company ESG lookup + portfolio rebalancing (core product, complex)
- **mi-web** — Investment dashboard with PDF/Excel/SVG exports (heaviest data)

**Migration order suggestion:** root-web → quiz-web → esg-web → mi-web (simple to complex)

### Questions for You

1. Is full SolidJS → relementjs rewrite acceptable, or should we keep SolidJS and only adopt rhonojs for the server layer?
Full rewrite
2. Auth0 migration strategy — keep Auth0, or fully switch to Persontric?
Keep auth0
3. Deployment target — Cloudflare Workers or Bun server?
Cloudflare Workers. Use cloudflare dev environment. If convenient, also support Bun server. But it's a lower priority.
4. Which app to start with?

- root-web (easiest)
- mi-web
- esg-web
<!-- /agent:exchange -->
