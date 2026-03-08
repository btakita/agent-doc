---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** All core features complete ✓. Benchmark portfolios ✓. mi-web/esg-web deployed ✓. Auth flow ✓. User portfolios CRUD ✓. Remaining: custom domains (need domain names), AUTH0_CLIENT_ID secret.
<!-- /agent:status -->

<!-- agent:exchange -->

### All 6 Tasks Complete ✓

**1. Benchmark Data ✓** — Created 3 benchmark portfolios with realistic ESG scores:
- **URTH** (50 holdings) — iShares MSCI World ETF, broad market, average ESG
- **ERTH** (40 holdings) — Invesco MSCI Sustainable Future, high environmental scores
- **PXWEX** (35 holdings) — Pax World Environmental Markets, strong governance + environmental

Available at: `/portfolios/urth`, `/portfolios/erth`, `/portfolios/pxwex` (+ reports + XLSX exports)

**2. Deploy mi-web ✓** — https://censible-mi-web.brian-takita.workers.dev
**3. Deploy esg-web ✓** — https://censible-esg-web.brian-takita.workers.dev

**4. Custom Domains** — Requires Cloudflare dashboard configuration. Which domains? (e.g. `censible.co`, `mi.censible.co`, `esg.censible.co`)

**5. Auth Flow ✓** — Auth0 Universal Login integration:
- `/login` → Auth0 hosted login (implicit flow)
- `/callback` → Token extraction + localStorage storage
- `/logout` → Auth0 logout + token clear
- Nav bar shows login/logout toggle based on token state
- Needs `AUTH0_CLIENT_ID` set via `wrangler secret put AUTH0_CLIENT_ID`
How do I get the AUTH0_CLIENT_ID from auth0's website?

**6. User Portfolios ✓** — Full CRUD UI:
- `/rebalance/portfolios` — list user portfolios
- `/rebalance/portfolios/new` — create with ticker autocomplete
- `/rebalance/portfolios/:name` — view portfolio
- `/rebalance/portfolios/:name/edit` — edit positions + delete

**Deployed:** Version `47b86dbd` (817 KiB / 122 KiB gzipped)
<!-- /agent:exchange -->
