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

### Comparison: Current vs censible.co vs mi-www.site

| Feature | censible.co (original) | mi-www.site | workers.dev (current) |
|---------|----------------------|-------------|----------------------|
| **Home/Landing** | ✓ Solutions pitch | Login page | ✓ Redirects to /solutions |
| **Solutions** | ✓ ESG methodology, products | — | ✓ Same content |
| **FAQ** | ✓ | — | ✓ Same content |
| **Team** | ✓ | — | ✓ Same content |
| **Legal** | ✓ | — | ✓ Same content |
| **Company Search** | ✓ esg.censible.co (A-Z list + search) | ✓ /companies | ✓ /companies (FTS5 search) |
| **Company Profile** | ✓ Full ESG scores | ✓ /companies/:ticker | ✓ Basic info + ESG teaser (tier) |
| **ESG Rankings Hub** | ✓ 12 issues | — | ✓ 12 issues + data tables |
| **WIL Rankings** | ✓ Companies + Funds | — | ✓ Companies only (funds empty) |
| **Portfolio Collections** | — | ✓ Eco/Global/Tortue/US/Wil | ✗ Not implemented |
Implement
| **Portfolio History** | — | ✓ /portfolios | ✗ Not implemented |
Implement
| **Rebalance/Portfolio** | ✓ (behind login) | — | ✓ API complete, UI placeholder |
| **Learn/Blog** | ✓ learn.censible.co | — | ✗ Not implemented |
| **Auth** | ✓ Auth0 | ✓ Login/password | ✓ Auth0 JWT |
| **Financial Data** | ✓ (via finegar API) | — | ✗ API down, no replacement |
| **Fund ESG Rankings** | ✓ Best/Worst/Largest/RI | — | ✗ No fund data |
Is it on the USB drive?
Is the Finegar DB credentials on the USB drive?
| **PDF/Excel Reports** | ✓ Custom reports | — | ✗ Not implemented |
Implement

**What we have (workers.dev):**
- ✓ All static pages (solutions, FAQ, team, legal)
- ✓ Company search (FTS5, better than original A-Z list)
- ✓ Company profiles with ESG teaser (tier only, no numeric scores publicly)
- ✓ ESG issue rankings with data tables (12 issues, top 50 companies each)
- ✓ WIL company rankings (3 categories, 65 companies)
- ✓ SP1500 CEO infographic
- ✓ Portfolio API (CRUD, behind auth)
- ✓ ESG data API (behind auth)

**What's missing:**
1. **Fund rankings** — 4 WIL fund sections + fund ESG data (need fund metadata source)
2. **Portfolio collections** — mi-www.site's Eco/Global/Tortue/US/Wil themed portfolios
3. **Financial data** — market cap, PE ratio, prices (need FMP or similar API)
4. **Learn/Blog** — learn.censible.co (separate subdomain, could be Cloudflare Pages)
5. **PDF/Excel reports** — custom ESG compliance reports
6. **Company A-Z listing** — esg.censible.co shows all companies browsable (we have search-only)

Priority for next steps?
<!-- /agent:exchange -->
