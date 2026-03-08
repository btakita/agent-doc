---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Phase 15 complete ✓ — Company A-Z, PDF/CSV reports, mi-web/esg-web stubs. Finnhub ✓. Portfolios ✓. WIL ✓. ESG teaser ✓. Auth ✓. Next: deploy mi-web/esg-web, learn/blog, custom domains.
<!-- /agent:status -->

<!-- agent:exchange -->

### Phase 15 — Company A-Z, PDF Reports, mi-web/esg-web ✓

Built all three in parallel agents and deployed:

**1. Company A-Z Browse** ✓
- `/companies/browse?letter=A` — alphabetical listing with A-Z nav
- Links from `/companies` search page
- Shows ticker, name, industry, country (500 per letter)
- **Test:** https://censible-root-web.brian-takita.workers.dev/companies/browse
What happened to search? Can we implement autocomplete search?

**2. PDF / CSV Reports** ✓
- `/companies/:ticker/report` — printer-friendly HTML report with "Print / Save as PDF" button
- Public: company info + financials + ESG tier. Authenticated: full ESG scores + gender data
- `/api/companies/:locator/report.pdf` — authenticated full report
- `/api/companies/:locator/export.csv` — CSV export of all ESG data
- `@media print` CSS for clean PDF output
- **Test:** https://censible-root-web.brian-takita.workers.dev/companies/AAPL/report

**3. mi-web / esg-web Stubs** ✓
- `apps/mi-web/` — Hono worker stub ("Censible Portfolio Tool"), D1 + KV bindings, builds clean (66 KiB)
- `apps/esg-web/` — Hono worker stub ("ESG Explorer"), search redirect, builds clean (67 KiB)
- Both have standalone layouts with cross-app nav (root/mi/esg)
- Ready to deploy when needed: `npx wrangler deploy` from each directory

**New files:** `pages/company_browse.ts`, `pages/report.ts`, `apps/mi-web/*`, `apps/esg-web/*`
**Modified:** `worker/index.ts` (3 new routes), `worker/middleware/auth0.ts` (optional auth helper)

All features deployed to root-web. What's next?

mi-www.site?
<!-- /agent:exchange -->
