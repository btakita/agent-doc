---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Decisions locked. Revised migration plan ready. Next: begin Phase 1 on root-web.
<!-- /agent:status -->

<!-- agent:exchange -->
## Migration Assessment: censible-core → rappstack/rhonojs

### Decisions (locked)

| Decision | Choice |
|----------|--------|
| UI framework | Full rewrite: SolidJS → relementjs |
| Auth | Keep Auth0 (wrap with Hono middleware) |
| Database | PostgreSQL → D1 (SQLite on Cloudflare) |
| Caching | Redis → Cloudflare KV (or Durable Objects if needed) |
| Deploy primary | Cloudflare Workers |
| Deploy secondary | Bun server (lower priority) |
| Migration order | root-web → mi-web → esg-web |

### Revised Migration Plan

**Phase 1 — root-web on rhonojs + Cloudflare Workers**
Simplest app first. Marketing/landing site with static-ish pages, no auth, no database.
- Scaffold new rhonojs app structure (build.ts, start.ts, worker/index.ts, wrangler.toml)
- Rewrite SolidJS pages to relementjs server-rendered HTML
- Port routes from Solid Start file-based routing to Hono handlers
- Build with `rhonojs_server__build()` + `rhonojs_browser__build()`
- Deploy to CF Workers via `wrangler deploy`
- Verify: all pages render, SEO meta preserved, styles working

**Phase 2 — Database migration (D1)**
Before tackling mi-web/esg-web, set up the data layer.
- Define Drizzle schemas matching existing PostgreSQL tables
- Create D1 database for each environment (prod, staging)
- Write PostgreSQL → D1 data migration scripts
- Adapt raw SQL queries to Drizzle ORM with D1 driver
- Test data integrity after migration

**Phase 3 — Auth0 on Cloudflare Workers**
Auth0 SDK works in Workers with some adaptation.
- Create Hono middleware for Auth0 JWT verification
- Port `/auth` OAuth redirect flow to Hono routes
- Session management: use CF KV for session storage (replacing in-memory/Redis)
- Port account creation/preferences to D1 queries
- Test full login → session → protected route flow

**Phase 4 — Caching layer (KV)**
Replace Redis with Cloudflare KV.
- Map existing Redis cache keys to KV namespace structure
- Port `@censible/redis-client` to a KV-backed cache client
- Financial data caching, FX rate caching → KV with TTL
- Browser/server split: KV only accessible server-side (same as Redis)

**Phase 5 — mi-web rewrite**
Investment dashboard — heavy data, PDF/Excel/SVG exports.
- Rewrite SolidJS components to relementjs
- Port API routes (data exports, portfolio endpoints) to Hono
- PDF generation: pdfkit/pdfmake may need adaptation for Workers (memory/CPU limits)
  - Alternative: use CF Workers Paid plan or offload to a separate service
I'm concerned about PDF generation memory usage. What are the costs of the CF Workers Paid plan?
- Excel/SVG generation: verify xlsx and svg libraries work in Workers runtime
- Charts/visualizations: server-render to SVG via relementjs

**Phase 6 — esg-web rewrite**
Core product — company ESG lookup + portfolio rebalancing.
- Most complex app: search, company profiles, portfolio CRUD
- Rewrite all SolidJS components to relementjs
- Port REST API (companies, portfolios, rebalance) to Hono routes
- Auth0-protected routes via Phase 3 middleware
- Data-intensive queries via Drizzle + D1

**Phase 7 — Remaining apps & cleanup**
- Evaluate which apps (quiz-web, msci-demo-web, clasp-web, kyc-web, finegar-etl, screenshot-web) to migrate vs. deprecate
- finegar-etl may need a different deployment (not CF Workers — long-running ETL jobs)
- Decommission Heroku after all apps migrated
- Remove Solid Start, Vite, itty-router dependencies

### Technical Notes

**Auth0 in CF Workers:**
- `auth0` npm package uses Node.js crypto — needs `nodejs_compat` compatibility flag in wrangler.toml
- Alternative: use Auth0's `/authorize` redirect + `/oauth/token` endpoint directly (HTTP-based, no SDK dependency)
- JWT verification via `jose` library (works natively in Workers)

**D1 limitations to watch:**
- 10GB max database size (per database)
- No stored procedures or advanced PostgreSQL features
- Row size limit: 1MB
- If data exceeds limits, consider splitting across multiple D1 databases or using Hyperdrive + external PostgreSQL

**PDF/Excel in Workers:**
- Workers have 128MB memory limit (paid plan) and 30s CPU time
- Large PDF generation may hit limits — consider:
  - R2 for storing generated files
  - Queue + Durable Object for async generation
  - Or keep a small Bun server for heavy export jobs

### Architecture Diagram (target state)

```
                    Cloudflare Edge
                    ┌─────────────────────────────────────┐
                    │                                     │
User ──→ CF DNS ──→ │  Workers (rhonojs/Hono)            │
                    │  ├── root-web (marketing)           │
                    │  ├── mi-web (dashboard)             │
                    │  └── esg-web (ESG platform)         │
                    │                                     │
                    │  D1 (SQLite) ← Drizzle ORM         │
                    │  KV ← Cache (replaces Redis)        │
                    │  R2 ← File storage (PDFs, exports) │
                    │                                     │
                    └──────────┬──────────────────────────┘
                               │
                          Auth0 (external)
                          JWT verification
```
<!-- /agent:exchange -->
