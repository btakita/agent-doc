---
agent_doc_session: 9ff575de-4c8b-4217-8b36-5287c77af629
tmux_session: '0'
---

Add a static site export feature.
This will generate *.html files.

Let's add a cloudflare worker export feature that uses the static site export feature. It will also add cloudflare worker(s) to handle dynamic content.

See brookebrodack-dev.md and briantakita.me-dev.md

## Assistant

### Current State

relysjs (`/home/brian/work/relysjs/relysjs/`, v1.21.73) is a library-first SSR framework built on Elysia.js/Bun with ESBuild. It has **no built-in CLI or static export mode**. Both brookebrodack-site and briantakita.me implement static export with per-app scripts that:

1. Build the app (`relysjs_browser__build` + `relysjs_server__build`)
2. Start the Elysia server locally
3. Fetch routes via HTTP
4. Rewrite localhost URLs to production domain
5. Write HTML files to `dist/browser/`

The pattern is nearly identical across both sites — a strong signal this should be framework-level.

### Proposed Design: `relysjs/export`

**Feature 1: Static Site Export** (`relysjs/server/export`)

A library module (not CLI — consistent with relysjs's library-first design) that apps call from their own `static-export.ts`:

```typescript
// relysjs/server/export
export async function static_export_(config: static_export_config_T): Promise<void>

interface static_export_config_T {
  routes: string[]              // Routes to pre-render (e.g., ['/', '/about'])
  site_url: string              // Production URL for URL rewriting
  out_dir?: string              // Output directory (default: 'dist/browser')
  port?: number                 // Server port (default: from app_ctx)
  app?: Elysia                  // Optional pre-started app
  extra_files?: string[]        // Non-HTML routes (robots.txt, sitemap.xml, rss.xml)
  url_rewrite?: boolean         // Enable localhost→site_url rewriting (default: true)
}
```

The function handles: start server (if not provided) → fetch each route → rewrite URLs → write files → stop server. This extracts the common pattern from both sites.

**Feature 2: Cloudflare Worker Export** (`relysjs/server/export/cloudflare`)

Builds on static export, adding a Cloudflare Workers entry point for dynamic routes:

```typescript
export async function cloudflare_export_(config: cloudflare_export_config_T): Promise<void>

interface cloudflare_export_config_T extends static_export_config_T {
  dynamic_routes?: string[]     // Routes handled by Worker (not static)
  worker_entry?: string         // Custom worker entry point
  wrangler_config?: object      // Wrangler.toml overrides
}
```

This would:
1. Run `static_export_()` for static routes
2. Generate a Cloudflare Worker entry that serves static files from KV/R2 and proxies dynamic routes to the Elysia app (compiled for Cloudflare Workers runtime)
3. Output a `wrangler.toml` configuration

### Questions Before Implementation

1. **Route discovery**: Should `static_export_` auto-discover routes from sitemap.xml (like briantakita.me) or require explicit route lists? Or both (explicit + sitemap fallback)?

Both.

2. **relysjs repo**: Should I work directly in `/home/brian/work/relysjs/relysjs/` or via the ctx-core submodule at `ctx-core-dev/lib/relysjs/`?

ctx-core-dev/lib/relysjs/

3. **Cloudflare Workers compatibility**: Elysia can run on Cloudflare Workers but with limitations (no Node.js APIs, no filesystem). Should the Worker export compile a separate lightweight handler, or attempt to run the full Elysia app on Workers?

Lightweight handler.
   
4. **Scope priority**: Should I start with just the static export library module and iterate, or design both features together?

both features

## User

