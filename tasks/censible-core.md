---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Portfolio collections live ✓ (9 portfolios). WIL complete ✓. ESG teaser ✓. Auth ✓. Finegar DB alive ✓. Next: Finnhub lazy fetch (Phase 14), PDF reports, mi-web/esg-web.
<!-- /agent:status -->

<!-- agent:exchange -->

### Phase 14 — Finnhub Lazy Fetch

**Approach:** Lazy fetch + KV cache (not cron). Data fetched on-demand when a user views a company, cached 24h in KV.

**Architecture:**
1. Add `FINNHUB_CACHE` KV binding + `FINNHUB_API_KEY` secret
2. On `/companies/:ticker` visit → check KV `finnhub:{ticker}`
3. Cache hit (< 24h): serve instantly
4. Cache miss: fetch Finnhub `/api/v1/quote` + `/api/v1/stock/metric`, store in KV with 24h TTL
5. Display: current price, market cap, PE ratio, EPS, 52-week high/low

**Why lazy > cron:**
- Zero cost for unviewed companies (most of 9K)
- Always fresh for actively-viewed ones
- 60 calls/min is plenty for on-demand traffic
- Simpler architecture (no scheduled worker)

Ready to implement — need your Finnhub API key (free signup at finnhub.io).

Setup FINNHUB_API_KEY in .env
<!-- /agent:exchange -->
