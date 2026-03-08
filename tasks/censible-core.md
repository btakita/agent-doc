---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Phase 1, Step 0 — pnpm → bun conversion in progress. Blocker: FontAwesome Pro token needed.
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

#### Step 0 — pnpm → bun conversion

**Done:**
- Converted `workspaces` from object format to flat array (bun-compatible)
- Moved `pnpm.overrides` → top-level `overrides`
- Removed `pnpm` from devDependencies
- Removed self-referencing `@censible/dev` devDependency
- Deleted `pnpm-workspace.yaml`
- Removed unused `vendor/*` workspace paths (no vendor dirs exist)

**Blocker: FontAwesome Pro auth token**

`bun install` fails because `@fortawesome/pro-*` packages require authentication against `npm.fontawesome.com`. The `.npmrc` references `FONTAWESOME_AUTH_TOKEN` but no `.env` file exists with the token.

censible-core/.env

The `.env.sample` has `FONTAWESOME_NPM_KEY=` (note the different var name).

To unblock:
1. Set `FONTAWESOME_AUTH_TOKEN=<your-token>` in `.env` or shell environment
2. Then run `bun install`

Alternatively, if FontAwesome Pro is no longer needed (switching to relementjs may eliminate icon dependencies), we could remove the `@fortawesome/pro-*` packages and the `.npmrc` registry config.

**Question:** Do you have the FontAwesome Pro token, or should we remove FA Pro dependencies and use an alternative (e.g., free FA, Heroicons, or inline SVGs)?
<!-- /agent:exchange -->
