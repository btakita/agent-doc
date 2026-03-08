---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Phase 1, Step 0 complete (pnpm → bun). Ready for Step 1: scaffold rhonojs app.
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

#### Step 1 — Scaffold rhonojs app (next)

Ready to clear existing Solid Start files from `apps/root-web/` and scaffold the rhonojs app structure. Proceeding now.

continue...finish rest of the steps
<!-- /agent:exchange -->
