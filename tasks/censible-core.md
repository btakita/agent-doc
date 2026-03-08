---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Phases 1–6 backend complete ✓ — All routes deployed. UI placeholders for esg-web/mi-web. Blocked on search API (Heroku maintenance mode).
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

- 12 ESG issues index + individual issue pages + WIL data page
- Blocked: Heroku search API in maintenance mode (503)

### Phase 2 — D1 Database ✓

- `censible-db` (ID: `0129a78b-a3a4-49b9-8ac4-931cc3f4b00a`, ENAM)
- 6 tables: account, portfolio, position, portfolio_version, staged_change_position, staged_portfolio
- Query modules: `worker/db/account.ts`, `worker/db/portfolio.ts`

### Phase 3 — Auth0 Middleware ✓

- `worker/middleware/auth0.ts` — jose JWT verification against Auth0 JWKS

### Phase 4 — API Routes ✓

- Full portfolio CRUD at `/api/rebalance/*` (authenticated)
- Account preferences at `/api/account/preferences`
- Search proxy at `/search/*`
- Financial data proxy: `/securities/:ticker/financial`, `/currencies/:code/fx_rate`

### Phase 5 — mi-web Assessment

**Decision: Defer full UI port.** mi-web generates static ESG reports (6 portfolios × 4 formats = 24 endpoints) from pre-computed 2022 data snapshots. Challenges:
- Source CSV data files not in repo (external data pipeline)
- Rendering uses SolidJS SSR → SVG → pdfkit → PDF pipeline
- XLSX uses archiver + custom SpreadsheetML XML
- All data is from fixed dates (2022-07-31, 2022-09-30, 2022-11-30)

**When data files are available:** Generate static HTML/SVG/PDF/XLSX and serve from `public/`

### Phase 6 — esg-web Routes ✓

**All esg-web URL routes now served by the Worker:**

| Route | Status |
|-------|--------|
| `/companies` | Placeholder (search form + "coming soon") |
| `/companies/:company_locator` | Placeholder (company name + back link) |
| `/rebalance` | 302 → `/rebalance/portfolios` |
| `/rebalance/portfolios` | Placeholder (login required note + API pointer) |
| `/rebalance/portfolios/new` | Placeholder |
| `/rebalance/portfolios/:name` | Placeholder |
| `/securities/:ticker/financial` | Proxy to search API |
| `/currencies/:code/fx_rate` | Proxy to search API |

**Backend complete, frontend placeholder.** The D1 portfolio API is fully functional. UI needs interactive components (company ESG charts, portfolio tables, CSV upload) — blocked on search API availability for company data.

### Current Architecture

```
censible-root-web (CF Worker, 217KB)
├── Static pages: /, /solutions, /faq, /team, /legal
├── Rankings: /company-etf-and-mutual-fund-rankings/*
├── Company: /companies, /companies/:locator (placeholder)
├── Portfolio: /rebalance/* (placeholder UI, full API)
├── API: /api/rebalance/* (D1 + Auth0)
├── Proxy: /search/*, /securities/*, /currencies/*
├── D1: censible-db (6 tables)
└── Assets: 217 static files from public/
```

### Remaining Work

| Task | Blocker | Priority |
|------|---------|----------|
| Company ESG detail pages (interactive) | Search API down | High |
I believe search is in https://github.com/portfoliome/finegar-api. Please add a Cloudflare compatible port of finegar-api search and other necessary features to censible-core. 
| Portfolio management UI (login, CRUD forms) | Auth0 credentials needed | High |
How do I get these Auth0 credentials? See screenshot.
| mi-web report generation | Data CSV files needed | Medium |
I believe that the CSV files are on Contabo. Either in the container or S3 bucket.
| KV caching layer | None (optimization) | Low |
| Decommission Contabo containers | All above complete | Last |

**Blockers:**
1. **Heroku search API is in maintenance mode** — WIL page, company detail, and financial proxies all return errors. Need to either restore the Heroku service or migrate the search API itself.
2. **Auth0 credentials** — AUTH0_DOMAIN and AUTH0_AUDIENCE need to be set as secrets (`wrangler secret put AUTH0_DOMAIN`, `wrangler secret put AUTH0_AUDIENCE`)
3. **mi-web data** — ESG report CSV source files not in the repository
<!-- /agent:exchange -->
