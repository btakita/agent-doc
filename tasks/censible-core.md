---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Finnhub financials live ✓ (lazy fetch + KV cache). Portfolio collections ✓. WIL ✓. ESG teaser ✓. Auth ✓. Next: PDF reports, mi-web/esg-web, Company A-Z.
<!-- /agent:status -->

<!-- agent:exchange -->

### Phase 14 — Finnhub Lazy Fetch ✓

Found existing API key in `.env`. Implemented and deployed:

**What's live:**
- Lazy fetch from Finnhub (quote + metrics) with 24h KV cache
- Company pages now show: price, change, market cap, P/E, EPS, 52W range, beta
- Public API: `/api/companies/:ticker/financials`
- KV namespace `FINNHUB_CACHE` + secret `FINNHUB_API_KEY` configured

**Architecture:**
- `worker/finnhub.ts` — fetch + cache module (2 API calls per ticker, cached 24h in KV)
- On company page load: ESG teaser + Finnhub data fetched in parallel
- No data fetched for companies without tickers
- Graceful degradation: if Finnhub returns nothing, financial section is hidden

**Test:** https://censible-root-web.brian-takita.workers.dev/companies/AAPL
**API:** https://censible-root-web.brian-takita.workers.dev/api/companies/AAPL/financials

What's next? Remaining items: PDF reports, mi-web/esg-web apps, Company A-Z listing.

Do it all in parallel agents.
<!-- /agent:exchange -->
