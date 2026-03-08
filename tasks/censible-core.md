---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Portfolio collections live ✓ (9 portfolios). WIL complete ✓. ESG teaser ✓. Auth ✓. Finegar DB alive ✓. Next: financial data (Finnhub), PDF reports, mi-web/esg-web.
<!-- /agent:status -->

<!-- agent:exchange -->

### Phase 13 — Portfolio Collections ✓

**Deployed 9 portfolio pages:**
- `/portfolios` — index page with Core, Eco, WIL groups
- `/portfolios/mi` — Censible core portfolio (26 holdings)
- `/portfolios/mi_eco_*` — 4 Eco portfolio snapshots (20-24 holdings each)
- `/portfolios/mi_wil_*` — 4 WIL portfolio snapshots (17 holdings each)

Each portfolio page shows: holdings table with ticker, name, weight, and 6 key ESG issue scores (carbon, labor, governance, privacy, gender, health & safety) color-coded by score level.

**Test:** https://censible-root-web.brian-takita.workers.dev/portfolios

**Free financial data APIs — research results:**

| API | Free tier | Rate limit | Viable for 9K? |
|-----|-----------|------------|-----------------|
| **Finnhub** | Yes, free key | 60/min | **Best** — 2.5 hrs for full refresh |
| **FMP** | 250/day | 250/day | No free (need $19/mo) |
| **yfinance** | No key needed | Aggressive blocking | Risky, ToS issues |
| **Alpha Vantage** | 25/day | 25/day | No (360 days!) |
| **TradingView** | None | — | No public API |
| **Google Finance** | Deprecated 2012 | — | No |
| **Yahoo Finance** | None official | — | No official API |

**Recommendation:** Finnhub free tier is the best option — 60 calls/min, no daily limit, covers market cap, PE ratio, price, EPS. A scheduled Cloudflare Worker cron could refresh all 9K companies every few hours.

Alternatively, FMP at $19/mo has bulk endpoints that can fetch all data in a few calls.

What's your preference: Finnhub (free) or FMP ($19/mo)?
Finnhub

Can we lazily fetch data?
<!-- /agent:exchange -->
