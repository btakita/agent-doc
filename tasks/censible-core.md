---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** WIL complete with fund data ✓ (from live finegar RDS DB). ESG teaser ✓. Auth ✓. Next: portfolio collections, financial data, PDF reports, mi-web/esg-web.
<!-- /agent:status -->

<!-- agent:exchange -->

### Phase 12 — Fund WIL Rankings + Finegar DB Discovery ✓

**Finegar DB is alive!** Found credentials on USB drive:
- Host: `finegar-large.cka6h4ihwghf.us-east-1.rds.amazonaws.com` (AWS RDS, not Heroku)
- 66 tables, including: `domestic_funds` (20K funds), `fund_company_exposures` (5.2M holdings), `board_leadership` (21K companies), `company_women_leadership`, `company_financial`, `market_cap`, etc.

**Fund WIL Rankings (deployed):**
Computed from finegar DB: joined `domestic_funds` → `fund_company_exposures` → `board_leadership` to aggregate per-fund gender metrics across 14,967 qualifying funds.
- Best 25 funds by WIL score (top: iShares MSCI Sweden ETF)
- Worst 25 funds (with 50+ holdings, bottom: First Trust Japan AlphaDEX)
- Largest 25 funds by number of holdings
- 25 Responsible Investing funds (ESG/SRI/sustainable keywords)
- All 4 fund sections now populated on WIL page

**Test:** https://censible-root-web.brian-takita.workers.dev/company-etf-and-mutual-fund-rankings/women-in-leadership

Thank you

**Financial data in finegar DB:**
`company_financial` table has market cap, PE ratio, price, dividend yield, etc. — but data is from 2016 (stale). For current data, best option is FMP API (free tier, 250 req/day with bulk endpoints).

Let's use the FMP API. Does TradingView, Yahoo Finance, Google Finance, or another company have free apis?

**Next steps — what I'll implement now:**
1. Portfolio collections (Eco/Global/Tortue/US/Wil) — the `_mi_*.csv` files have holdings, can build portfolio pages showing composition with ESG scores from D1
2. Portfolio history — list available portfolio dates from CSV files
3. PDF reports — generate ESG compliance PDFs using the data in D1 + finegar

Should I proceed with portfolio collections first?

Yes
<!-- /agent:exchange -->
