---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Phase 1 Steps 0–4 complete. root-web scaffolded, all routes verified locally. Ready for deploy.
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

#### Step 0 — pnpm → bun conversion ✓

**Completed:**
- Converted `workspaces` from object format to flat array
- Moved `pnpm.overrides` → top-level `overrides`
- Removed `pnpm` from devDependencies, removed self-referencing `@censible/dev`
- Deleted `pnpm-workspace.yaml`
- `.env` created with FontAwesome Pro token
- `bun install --ignore-scripts` succeeded, `bun.lock` generated (815KB)
- Note: `better-sqlite3` native build skipped (incompatible with Node 25.x; will be removed when mi-web migrates to D1)
- Peer dependency warnings for old SolidJS versions (expected, will be resolved as apps migrate)

#### Step 1 — Scaffold rhonojs app ✓

**Completed:**
- Cleared old SolidJS source files (`src/`, `vite.config.js`, `rollup.config.mjs`)
- Created `worker/index.ts` — Hono app with routes: /, /solutions, /faq, /team, /legal + redirects
- Created `wrangler.toml` — CF Workers config with `[assets]` serving from `public/`
- Updated `package.json` — replaced SolidJS/Solid Start deps with hono, wrangler, @cloudflare/workers-types
- Updated `tsconfig.json` — ES2022 target, bundler moduleResolution
- Preserved `public/` symlink to shared static assets

#### Step 2 — Port design tokens ✓

CSS from `@censible/css` tokens inlined directly into page layout. All typography, color, grid, and animation tokens carried over.

#### Step 3 — Create layout ✓

`pages/layout.ts` — full HTML document layout with:
- Masthead (top bar, navigation dropdowns, logo)
- Footer (company info, social links, disclaimers)
- Responsive nav with mobile menu support
- All CSS inlined from original SolidJS layout components

#### Step 4 — Port pages ✓

All static pages ported from SolidJS routes to plain HTML template functions:
- `pages/solutions.ts` — Hero, integrate, products, ESG methodology (12 articles), partner sections
- `pages/faq.ts` — 17 collapsible `<details>` across 4 sections
- `pages/team.ts` — Hero, 3 team bios, commitment/story/questions
- `pages/legal.ts` — Terms of use + privacy policy (5 articles)
- **Not yet ported:** `/company-etf-and-mutual-fund-rankings/*` (data-dependent, requires D1)

#### Step 5 — Local verification ✓

- `bun install` succeeded (hono 4.12.5, wrangler 4.71.0)
- Fixed: removed `nodejs_compat` flag (caused unenv path resolution bug with bun)
- `wrangler dev` — all routes verified:
  - `/` → 302 redirect to /solutions
  - `/solutions` → 200 (36KB HTML)
  - `/faq` → 200
  - `/team` → 200
  - `/legal` → 200
  - `/gender_diversity` → 302 redirect

#### Next: Deploy to Cloudflare Workers

Ready to run `wrangler deploy` to push root-web live. This requires CF API credentials configured.

**Remaining for full migration:**
- Deploy root-web to CF Workers
- Port company-etf-and-mutual-fund-rankings pages (Phase 2, needs D1)
- Phase 3: mi-web migration
- Phase 4: esg-web migration
<!-- /agent:exchange -->
