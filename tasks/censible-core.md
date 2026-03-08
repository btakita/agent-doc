---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Core features complete ✓ (XLSX, SVG, PDF, search, Finnhub). Remaining: benchmark data, mi-web/esg-web deploy, custom domains, auth flow, user portfolios.
<!-- /agent:status -->

<!-- agent:exchange -->

### Migration Status

XLSX fixes confirmed working. Here's what remains:

| # | Task | Effort | Notes |
|---|------|--------|-------|
| 1 | **Benchmark Data** | Medium | Import URTH/ERTH/PXWEX portfolios, add benchmark comparison to report page |
| 2 | **Deploy mi-web** | Low | Stub worker ready, just `wrangler deploy` + routes |
| 3 | **Deploy esg-web** | Low | Same as mi-web |
| 4 | **Custom Domains** | Low | Configure Cloudflare custom domains for each worker |
| 5 | **Auth Flow** | Medium | Auth0 login/signup UI (middleware exists, no login page yet) |
| 6 | **User Portfolios** | High | CRUD for custom portfolios (currently static JSON only) |

Core feature parity is solid: ESG data, search, company browse, reports, SVG charts, PDF export, detailed XLSX export, Finnhub financials. Ready to proceed with any of these — which would you like to tackle next?

Do all of the tasks.
<!-- /agent:exchange -->
