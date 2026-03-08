---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Phase 1 in progress — ready to start. Step 0: pnpm → bun conversion. Then scaffold root-web with rhonojs.
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
| Package manager | bun (convert from pnpm) |
| App directory | Overwrite root-web in place |
| CSS tokens | Keep @censible/css as shared lib |

### Phase 1 — root-web Migration

#### Step 0 — pnpm → bun workspace conversion
- Add `"workspaces"` field to root `package.json` (replaces `pnpm-workspace.yaml`)
- Delete `pnpm-workspace.yaml` and `pnpm-lock.yaml`
- Run `bun install` to generate `bun.lock`
- Convert `workspace:*` references to `workspace:*` (bun uses same syntax — no change needed)
- Verify all packages resolve correctly

#### Step 1 — Scaffold rhonojs app in `apps/root-web/`
Clear existing Solid Start files, create new structure:
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
├── package.json           # Updated deps (remove solid-*, add rhonojs/hono/relementjs)
└── public/                # Keep existing static assets
```

#### Step 2 — Port design tokens
Keep `@censible/css` as shared lib. It exports plain constants (colors, font names, breakpoints) — no SolidJS dependency.

#### Step 3 — Create relementjs layout
Port `Layout_nav_footer_header` → `layout__doc_html_()`:
- `<head>` with meta tags, fonts, favicons
- Navigation header (masthead)
- Footer with nav columns
- Wrap page content

#### Step 4 — Port pages
1. `/solutions` — hero, product cards, ESG methodology grid
2. `/faq` — collapsible Q&A (`<details>`)
3. `/team` — expandable team bios with photos
4. `/legal` — Terms & Privacy Policy
5. `/company-etf-and-mutual-fund-rankings/*` — ESG rankings (data-dependent)

#### Step 5 — Build & deploy
- `bun build.ts` → rhonojs server + browser builds
- `bun start.ts` → local dev
- `wrangler deploy` → CF Workers

Ready to begin. Say "go" and I'll start with Step 0 (pnpm → bun conversion).

go
<!-- /agent:exchange -->
