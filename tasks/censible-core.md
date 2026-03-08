---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** ESG data moved behind Auth0 login ✓. Public: search + basic company info. Auth required: ESG scores, MSCI ratings, gender data.
<!-- /agent:status -->

<!-- agent:exchange -->

### Phase 9 — ESG Data Auth Protection ✓

Moved all ESG score data behind Auth0 login per licensing requirements:

**Public (no auth):**
- `GET /api/search?q=...` — company name/ticker search
- `/companies?q=...` — search page with results table
- `/companies/:ticker` — basic company info (name, ticker, exchange, industry, description) + "login to view ESG data" prompt

**Auth required (401 without JWT):**
- `GET /api/companies/:locator` — full company detail with ESG + MSCI + gender
- `GET /api/companies/:locator/esg` — ESG key issues
- `GET /api/companies/:locator/msci` — MSCI ratings
- `GET /api/companies/:locator/gender` — gender diversity

**Live endpoints:**
- `GET /api/search?q=apple&limit=5` — FTS company search
- `GET /api/companies/AAPL` — Full company detail (ESG + MSCI + gender)
- `GET /api/companies/AAPL/esg` — ESG key issues only
- `GET /api/companies/AAPL/msci` — MSCI ratings only
- `GET /api/companies/AAPL/gender` — Gender diversity only

**Live pages:**
- `/companies?q=apple` — Company search with results table (ticker, name, industry, exchange)
- `/companies/AAPL` — Company ESG profile page with:
  - MSCI ESG summary cards (Overall, E, S, G scores)
  - Gender diversity stats (female directors, executives, workforce %)
  - ESG key issues table (35 issues x score + percentile)
  - Company description and metadata

**Test:** https://censible-root-web.brian-takita.workers.dev/companies/AAPL

### Current Architecture (updated)

```
censible-root-web (CF Worker, 225KB)
├── Static pages: /, /solutions, /faq, /team, /legal
├── Rankings: /company-etf-and-mutual-fund-rankings/*
├── Company search: /companies?q=... (FTS5, live)
├── Company detail: /companies/:ticker (ESG + MSCI + gender, live)
├── Portfolio: /rebalance/* (placeholder UI, full API)
├── API (public): /api/search, /api/companies/:locator
├── API (auth): /api/rebalance/* (D1 + Auth0)
├── D1: censible-db (12 tables, 9K companies + portfolio data)
└── Assets: 217 static files from public/
```

**Remaining work:**
- WIL rankings page needs data source (search API is down — could load from D1 or static JSON)
Please implement the search API
- mi-web / esg-web apps (phases 5, 6 — deferred)
Please implement
- Fund data import (if needed)
Please implement

The direct company esg scores cannot be publicly accessible due to licensing reasons.
Please put the esg scores behind a login.

Perhaps we can keep the public profile of each company but the scores must be behind a login.
<!-- /agent:exchange -->
