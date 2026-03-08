---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Phase 1 started — root-web migration. Exploring current structure and planning scaffolding.
<!-- /agent:status -->

<!-- agent:exchange -->
## Migration Assessment: censible-core → rappstack/rhonojs

### Decisions (locked)

| Decision | Choice |
|----------|--------|
| UI framework | Full rewrite: SolidJS → relementjs |
| Auth | Keep Auth0 (wrap with Hono middleware) |
| Database | PostgreSQL → D1 (SQLite on Cloudflare) |
| Caching | Redis → Cloudflare KV (or Durable Objects if needed) |
| Deploy primary | Cloudflare Workers |
| Deploy secondary | Bun server (lower priority) |
| Migration order | root-web → mi-web → esg-web |

### Phase 1 — root-web Migration

#### Current root-web Structure

**Pages (8 routes):**
- `/` → redirect to `/solutions`
- `/solutions` — main product/marketing page (hero, product cards, ESG methodology grid)
- `/faq` — collapsible Q&A with `<details>` elements
- `/team` — expandable team bios with photos
- `/legal` — Terms & Privacy Policy
- `/company-etf-and-mutual-fund-rankings` — ESG ranking index
- `/company-etf-and-mutual-fund-rankings/women-in-leadership` — WIL data (only page with server data fetch)
- `/company-etf-and-mutual-fund-rankings/:issue_id` — dynamic ESG issue pages
- Server redirects: `/gender_diversity`, `/women-in-leadership` → WIL page

**Layout:** `Layout_nav_footer_header` wrapping `Masthead_layout` (nav) + content + `Footer_layout`

**Styling:** CSS-in-JS via `@censible/css` tokens (colors, typography, breakpoints). Custom fonts: Neuzeitgro, PT Serif. No Tailwind.

**Dependencies on monorepo libs:**
- `@censible/ui-solid` — Layout, ESG_Root_Ranking_Page, WIL components
- `@censible/css` — Design tokens
- `@censible/domain` — Constants, issue definitions
- `@censible/service-domain` — wil_data_() for WIL page
- `@censible/ctx` — Context management

**Static assets:** Images (hero backgrounds, team photos, product illustrations), fonts, favicons, manifest.json

#### Migration Plan for root-web

**Step 1 — Scaffold rhonojs app structure**
Create new app directory (e.g., `apps/root-web-rhonojs/` or overwrite `apps/root-web/`):
```
root-web/
├── app/
│   ├── index.ts           # Hono app setup
│   ├── index.server.ts    # Routes → page functions
│   ├── index.browser.ts   # Browser hyop (nav toggle)
│   └── index.css          # Global styles
├── worker/
│   └── index.ts           # CF Worker entry
├── build.ts               # rhonojs_server__build + rhonojs_browser__build
├── config.ts              # Site metadata
├── start.ts               # Dev/prod server
├── wrangler.toml          # CF Workers config
├── package.json
└── public/                # Static assets (images, fonts, favicons)
```

**Step 2 — Port design tokens**
Migrate `@censible/css` tokens to work with relementjs. Options:
- Keep `@censible/css` as-is (it's just constants, not SolidJS-dependent)
- Or inline into app's CSS

**Step 3 — Create relementjs layout**
Port `Layout_nav_footer_header` → `layout__doc_html_()` function:
- `<head>` with meta tags, fonts, favicons
- Navigation header
- Footer with nav columns
- Wrap page content

**Step 4 — Port pages one at a time**
Each SolidJS page component → relementjs page function:
1. `/solutions` (most content, good for establishing patterns)
2. `/faq` (simple HTML, `<details>` elements)
3. `/team` (image-heavy, expandable cards)
4. `/legal` (pure text)
5. `/company-etf-and-mutual-fund-rankings/*` (data-dependent, last)

**Step 5 — Build & deploy**
- Configure build.ts with rhonojs plugins
- Test locally with `bun start.ts`
- Deploy to CF Workers via `wrangler deploy`

#### Questions Before Starting

1. Should we create a new app directory (`root-web-rhonojs`) alongside the existing one, or overwrite `root-web` in place?
In place is fine if it works.
2. The censible-core repo is a pnpm monorepo — should the new root-web stay in this monorepo or move to a separate repo?
Convert to bun.
3. Should we keep `@censible/css` as a shared lib or inline the design tokens?
shared is fine
<!-- /agent:exchange -->
