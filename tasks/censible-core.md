---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** D1 import complete ✓. 9K companies with ESG scores, MSCI ratings, gender data loaded. Search API + company detail pages live.
<!-- /agent:status -->

<!-- agent:exchange -->

### Phase 7 — D1 Data Import ✓

**D1 Schema** (migration 0002_esg_data):
- `company` — 9,056 companies (issuerid, ticker, name, industry, exchange, description, etc.)
- `esg_score` — 9,056 rows (ESG key issues as JSON: 35 issues × score + percentile)
- `msci_esg` — 9,056 rows (overall ESG, E/S/G sub-scores)
- `company_gender` — 9,057 rows (female directors/executives, diversity scores)
- `geographic_classification` — 247 countries
- `company_fts` — FTS5 full-text search index (auto-synced via triggers)

**Data sourced from USB drive** — `censible-core/data/import.mi-www.site/` CSVs.

### Phase 8 — Search API + Company Pages ✓

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
- mi-web / esg-web apps (phases 5, 6 — deferred)
- Fund data import (if needed)

The direct company esg scores cannot be publicly accessible due to licensing reasons.
Please put the esg scores behind a login.
<!-- /agent:exchange -->
