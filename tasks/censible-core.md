---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Migration complete ✓. All core features deployed. Minor UI polish gaps (executives, peers, contact info). Remaining setup: custom domains, Auth0 callback URLs.
<!-- /agent:status -->

<!-- agent:exchange -->

### Migration Status Assessment

**Core migration is complete.** All primary features from mi-www.site are replicated:

| Feature | Old (SolidJS) | New (Hono/CF Workers) | Status |
|---------|--------------|----------------------|--------|
| Company search | esg-web | `/companies`, `/api/search` | ✓ |
| Company detail + ESG | esg-web | `/companies/:locator` | ✓ |
| Portfolio collections | mi-web | `/portfolios/:id` | ✓ |
| Portfolio CRUD | esg-web | `/rebalance/portfolios/*` | ✓ |
| Rankings (WIL + issues) | root-web | `/company-etf-and-mutual-fund-rankings/*` | ✓ |
| Auth (Auth0) | esg-web | `/login`, `/callback`, `/logout` | ✓ |
| XLSX export (simple + detailed) | mi-web | `/portfolios/:id/export*.xlsx` | ✓ |
| SVG charts + PDF | mi-web | `/portfolios/:id/report` | ✓ |
| CSV export | esg-web | `/api/companies/:locator/export.csv` | ✓ |
| Finnhub financials | new | `/api/companies/:ticker/financials` | ✓ |
| Benchmark portfolios | new | URTH/ERTH/PXWEX | ✓ |
| Company browse (A-Z) | new | `/companies/browse` | ✓ |

**Minor gaps** (secondary UI polish, not blocking):
- Company executives table
- Company peers comparison on detail page
- HQ/contact info display
- Interactive color bar navigation
- Historical portfolio URL aliases

**Remaining setup:**
- Custom domains (Cloudflare dashboard)
- Auth0 callback URL configuration

I used a different method of auth for mi-www.site vs censible.co. mi-www.site is more constrained. Please use the same method of auth
<!-- /agent:exchange -->
