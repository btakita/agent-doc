---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** WIL rankings + ESG issue rankings live from D1 ✓. ESG scores behind auth ✓. Next: financial indicators, ESG teaser, fund data, mi-web/esg-web.
<!-- /agent:status -->

<!-- agent:exchange -->

### Phase 10 — WIL Rankings & ESG Issue Rankings ✓

Replaced the down CENSIBLE_SEARCH_URL with D1-backed queries:

**WIL Rankings (live):**
- Best 25 companies by gender diversity score
- Worst 15 (lowest gender score, NYSE/NASDAQ)
- Top 25 large-cap (NYSE/NASDAQ)
- SP1500 CEO infographic (bundled at build)
- Fund sections: empty (no fund-level data in D1 yet)

**ESG Issue Rankings (live):**
- 12 issues: carbon, governance, privacy, labor, etc.
- Top 50 companies per issue with score + percentile
- Linked to company profiles

**New public API:**
- `GET /api/rankings/wil` — WIL company rankings
- `GET /api/rankings/:issue_id` — issue-specific rankings

**Test:**
- https://censible-root-web.brian-takita.workers.dev/company-etf-and-mutual-fund-rankings/women-in-leadership
- https://censible-root-web.brian-takita.workers.dev/company-etf-and-mutual-fund-rankings/carbon_emissions
- https://censible-root-web.brian-takita.workers.dev/api/rankings/wil

**Re: financial indicators + ESG teaser:**
Can add to the public company profile page:
- Financial indicators (need data source — `_yahoo__equity.csv` or `_ft__security_detail.csv`)
- ESG teaser: overall tier (Leader/Average/Laggard) or letter rating without detailed scores
What specific financial indicators and what level of ESG teaser would you like?

**Remaining work:**
- Financial indicators + ESG teaser on public company page
- Fund data import (CSV files are portfolio holdings, not fund-level rankings — need fund metadata)
- mi-web / esg-web apps (separate deployments, need planning)
<!-- /agent:exchange -->
