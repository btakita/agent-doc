---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Phases 1–6 backend complete. Finegar-api analyzed (50+ endpoints). Plan: Hyperdrive → Contabo PostgreSQL. Need: Auth0 audience, PG external access.
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

### Phase 2 — D1 Database ✓

### Phase 3 — Auth0 Middleware ✓

- AUTH0_DOMAIN set to `censible.us.auth0.com` ✓
- AUTH0_AUDIENCE — **still needed** (see below)

### Phase 4 — API Routes ✓

### Phase 5 — mi-web Assessment (deferred)

### Phase 6 — esg-web Routes ✓ (placeholders)

### Phase 7 — Finegar-API Migration (in progress)

**Analyzed `portfoliome/finegar-api`** — Flask/Python API with 50+ endpoints. Key findings:

**Architecture:** Flask → PostgreSQL (finegar DB) + Redis cache + Celery tasks + IEX/Bloomberg pricing

**Endpoint categories:**
1. Company search/lookup (FTS on `security_search` table)
2. Company ESG ratings (`esg.key_issue_metrics`)
3. Women in Leadership rankings (`company_women_leadership_stats`, `women_leader_fund_scores`)
4. Fund ESG ratings (weighted by `fund_company_exposures`)
5. Company executives (`factset_screens.people`, `.jobs`, `.compensations`)
6. Company peers (`company_peers` with scoring)
7. Security pricing (IEX/Bloomberg with Redis cache)
8. Company screening (dynamic filter builder)
9. Reference data (countries, currencies, exchanges, geographic regions)

**Recommended approach: Cloudflare Hyperdrive**

Hyperdrive is Cloudflare's managed PostgreSQL proxy — it lets Workers make SQL queries directly to an existing PostgreSQL database. Benefits:
- No data migration needed — all 30+ tables and complex queries (FTS, window functions, CTEs) work as-is
- Connection pooling built-in
- ~30ms added latency
- Keep existing finegar schema intact

**Requirements to proceed:**
1. Expose PostgreSQL on Contabo externally (port 5432 or SSH tunnel)
2. Create Hyperdrive config: `wrangler hyperdrive create censible-finegar --connection-string "postgres://user:pass@host:5432/finegar"`

Let's port the search data to SQLite

### Action Items for User

**1. Auth0 Audience (blocking API auth)**

In the Auth0 dashboard (https://manage.auth0.com/dashboard/us/censible/):
- Go to **Applications → APIs** (left sidebar)
- Find the Censible API entry
- Copy the **Identifier** field (looks like `https://api.censible.co` or similar)
- Then I'll run: `wrangler secret put AUTH0_AUDIENCE`
AUTH0_AUDIENCE=https://censible.auth0.com/api/v2/

**2. Contabo PostgreSQL Access (blocking search)**

The finegar database needs to be accessible from Cloudflare Workers. Options:

**Option A (recommended): Expose PG directly**
- SSH into Contabo (207.244.234.186)
- Find the PostgreSQL port: `docker exec mi-www_site-1 env | grep PG`
- Add a port mapping or expose 5432 externally
- Then create Hyperdrive: `wrangler hyperdrive create censible-finegar --connection-string "postgres://user:pass@207.244.234.186:5432/finegar"`

**Option B: SSH tunnel**
- Use Cloudflare Tunnel (cloudflared) to expose PG securely without opening ports

**Option C: pg_dump + D1 import**
- Export key tables as JSON/CSV from Contabo
- Import into D1 (loses FTS and complex query support)

Option 3

**3. mi-web CSV Data**

If on Contabo, find them:
```bash
ssh root@207.244.234.186
docker exec mi-www_site-1 find / -name "esg_mi_*.csv" 2>/dev/null
```
Or check S3/object storage configuration.

Please look.

### Current Architecture

```
censible-root-web (CF Worker, 217KB)
├── Static pages: /, /solutions, /faq, /team, /legal
├── Rankings: /company-etf-and-mutual-fund-rankings/*
├── Company: /companies, /companies/:locator (placeholder)
├── Portfolio: /rebalance/* (placeholder UI, full API)
├── API: /api/rebalance/* (D1 + Auth0)
├── Proxy: /search/*, /securities/*, /currencies/*
├── D1: censible-db (6 tables, user/portfolio data)
└── Assets: 217 static files from public/
```

**Next:** Once PG access is configured → add Hyperdrive binding → implement finegar search endpoints directly in Worker → replace placeholders with live data.
<!-- /agent:exchange -->
