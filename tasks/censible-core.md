---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Phases 1–4 complete ✓ — D1 database, Auth0 middleware, API routes, and search proxy all deployed. Ready for Phase 5: mi-web UI migration.
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
| Package manager | bun (convert from pnpm) |
| App directory | Overwrite root-web in place |
| CSS tokens | Keep @censible/css as shared lib |

### Phase 1 — root-web Static Pages ✓

**Live URL:** https://censible-root-web.brian-takita.workers.dev

### Phase 1.5 — Rankings Pages ✓

- `pages/rankings.ts` — 12 ESG issues index + individual issue pages
- `pages/wil.ts` — Women in Leadership (fetches from search API, 11 sections)
- Note: Heroku search API returning 503 — WIL graceful error until available

### Phase 2 — D1 Database ✓

- Created D1 database `censible-db` (ID: `0129a78b-a3a4-49b9-8ac4-931cc3f4b00a`, region: ENAM)
- `migrations/0001_initial/up.sql` — 6 tables migrated from PostgreSQL:
  - `account`, `portfolio`, `position`, `portfolio_version`, `staged_change_position`, `staged_portfolio`
  - BIGSERIAL → INTEGER AUTOINCREMENT, MONEY → REAL, timestamps → ISO 8601 TEXT
- Schema applied to remote D1 (8 queries, 21 rows written)
- D1 binding added to `wrangler.toml`
- Query modules: `worker/db/account.ts`, `worker/db/portfolio.ts`

### Phase 3 — Auth0 Middleware ✓

- `worker/middleware/auth0.ts` — Hono middleware using `jose` for JWT verification
- Verifies against Auth0 JWKS endpoint, extracts email from JWT claims
- Supports both `email` and `https://censible.co/email` claim paths
- JWKS cached in module scope for performance
- AUTH0_DOMAIN and AUTH0_AUDIENCE configured as secrets

### Phase 4 — API Routes & Search Proxy ✓

**Authenticated API routes** (mounted at `/api/*`):
- `PUT /api/account/preferences` — Update account preferences
- `GET /api/rebalance/portfolio_names` — List portfolio names
- `GET /api/rebalance/portfolios/:name` — Get portfolio with positions
- `POST /api/rebalance/portfolios` — Create portfolio (with positions + versioning)
- `PATCH /api/rebalance/portfolios/:name` — Update (staged changes)
- `POST /api/rebalance/portfolios/:name/portfolio_version` — Save version snapshot
- `DELETE /api/rebalance/portfolios/:name` — Delete portfolio
- `GET /api/rebalance/portfolios/:name/card` — Portfolio card

**Search proxy** (unauthenticated):
- `/search/*` → proxies to `CENSIBLE_SEARCH_URL` (company/fund lookups)

**Deployed:** 212KB bundle, verified on production (401 for unauth API, 200 for static pages)

### Remaining Work

| Phase | Task | Status |
|-------|------|--------|
| 5 | mi-web UI migration (reports/exports) | Pending |
| 6 | esg-web UI migration (portfolio dashboard) | Pending |
| 7 | Decommission Contabo censible containers | Pending |

**Phase 5-6 scope:** Port SolidJS SPAs to server-rendered HTML. esg-web is the larger app (company lookup, portfolio management, ESG ratings, screening). mi-web has ESG report exports (PDF/SVG/XLSX).
<!-- /agent:exchange -->
