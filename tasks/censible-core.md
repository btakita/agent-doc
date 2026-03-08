---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Phase 1.5 complete ✓ — rankings pages deployed. Starting Phase 2: D1 database setup.
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

### Phase 1 — root-web Migration ✓

#### Step 0 — pnpm → bun conversion ✓
#### Step 1 — Scaffold rhonojs app ✓
#### Step 2 — Port design tokens ✓
#### Step 3 — Create layout ✓
#### Step 4 — Port pages ✓
#### Step 5 — Build, verify & deploy ✓

**Live URL:** https://censible-root-web.brian-takita.workers.dev

### Phase 1.5 — Rankings Pages ✓

- `pages/rankings.ts` — ESG root ranking index (12 issues as grid links) + individual issue pages
- `pages/wil.ts` — Women in Leadership data-driven page with 11 sections (hero, company tables, fund tables, methodology, SP1500 infographic)
- Worker routes: `/company-etf-and-mutual-fund-rankings`, `/:issue_id`, `/women-in-leadership`
- WIL fetches live data from `CENSIBLE_SEARCH_URL` (Heroku search API) — 7 parallel API calls
- SP1500 CEO data loaded from static JSON in `public/`
- `CENSIBLE_SEARCH_URL` secret configured on CF Workers
- Note: Heroku search API currently returning 503 — WIL page shows graceful error. Will work when API is available.
- All other rankings routes verified live (200)

### Phase 2 — D1 Database (in progress)

**PostgreSQL Schema (6 tables to migrate):**

| Table | Purpose | Complexity |
|-------|---------|------------|
| `account` | User preferences (email PK, JSON prefs) | Simple |
| `portfolio` | Portfolio metadata (BIGSERIAL PK, FK to account) | Simple |
| `position` | Holdings (composite PK, FK to portfolio) | Medium |
| `portfolio_version` | Historical snapshots (JSON storage) | Medium |
| `staged_change_position` | Working copy of positions | Medium |
| `staged_portfolio` | Working portfolio state | Simple |

**SQLite migration notes:**
- BIGSERIAL → INTEGER PRIMARY KEY AUTOINCREMENT
- MONEY → REAL
- TIMESTAMP WITH TIME ZONE → TEXT (ISO 8601)
- JSON → TEXT (SQLite JSON1 functions)
- ON DELETE CASCADE supported in SQLite with PRAGMA foreign_keys=ON

### Remaining Work

| Phase | Task | Status |
|-------|------|--------|
| 2 | D1 database + schema migration | In Progress |
| 3 | Auth0 integration on CF Workers (jose JWT middleware) | Pending |
| 4 | KV caching layer (replacing Redis) | Pending |
| 5 | mi-web migration | Pending |
| 6 | esg-web migration | Pending |
| 7 | Decommission Contabo censible containers | Pending |
<!-- /agent:exchange -->
