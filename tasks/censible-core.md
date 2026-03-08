---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** ESG teaser live ✓. WIL + issue rankings ✓. Auth ✓. Next: fund data import, financial indicators (FMP API), mi-web/esg-web.
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
Missing data for:
  - Best Funds for Women in Leadership
  - Worst Funds for Women in Leadership
  - Largest Funds — Women in Leadership
  - Responsible Investing Funds — Women in Leadership

**ESG Issue Rankings (live):**
- 12 issues: carbon, governance, privacy, labor, etc.
- Top 50 companies per issue with score + percentile
- Linked to company profiles

### Phase 11 — ESG Teaser + Research

**ESG Teaser (deployed):**
Public company pages now show ESG tier without numeric scores:
- Overall: Leader / Average / Laggard (based on MSCI ESG score 7+/4+/<4)
- E, S, G pills with same tier labels
- "Log in for detailed scores" CTA
- Test: https://censible-root-web.brian-takita.workers.dev/companies/AAPL
Teaser

**Free financial APIs:**
Best option: **Financial Modeling Prep (FMP)** — free tier, 250 req/day, has bulk endpoints (stock screener returns many tickers per call). Covers market cap, PE ratio, price, volume, sector. Would need a free API key. Can cover 9K companies with bulk calls well under the daily limit. Runner-up: Finnhub (60 req/min, no bulk). Alpha Vantage too slow (25/day).
Is there a free api for current data?

**Fund data — what's needed:**
The CSV files we have (`_mi_wil_*.csv`) are portfolio holdings (28 positions with tickers + weights), NOT fund-level rankings. For WIL fund sections we need:
1. **Fund metadata** — fund_name, ticker, fund_type, style, PE ratio, number_of_companies_in_fund
2. **Aggregated WIL metrics per fund** — female_board_pct, female_ceos/total_ceos, zero_female_board_members, board_>30%_female
3. **Fund categorization** — which funds are "largest", "responsible investing", etc.

This was originally computed by finegar-api (now down). Options:
- A) Import from a fund data source (Morningstar, FMP fund endpoint, etc.)
- B) Compute from holdings: use _mi_wil_*.csv holdings → look up each company's gender data in D1 → aggregate per fund. But we only have 28 holdings, not thousands of real funds.
- C) Use static curated data (manually list top/worst/largest/RI funds with their WIL metrics)
what files do we need?

**mi-web / esg-web — plan:**
These are separate Cloudflare Worker deployments. mi-web is a "mutual fund intelligence" dashboard showing fund-level ESG analysis. esg-web is an ESG-focused company analysis tool. Both currently exist as SolidJS apps in the monorepo. Migration would follow the same pattern as root-web: Hono worker + D1 + server-rendered HTML.
continue

Compare what we currently have with censible.co and mi-www.site
<!-- /agent:exchange -->
