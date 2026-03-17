---
agent_doc_session: 7322672d-726a-47c7-9a81-a836b968bdc9
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

## Current Architecture

**briantakita.me** is a pre-rendered static site deployed to **Cloudflare Pages**, built with **Hono** (via rhonojs) + **esbuild** (via rebuildjs).

| Component | Details |
|-----------|---------|
| Framework | Hono (rhonojs) — migrated from Elysia (relysjs) on 2026-03-14 |
| Hosting | Cloudflare Pages (`briantakita-me` project) |
| DNS | `briantakita.me` → Cloudflare Pages |
| Assets | R2 bucket `briantakita-assets` |
| R2 credentials | `pass briantakita.me/r/` (read), `pass briantakita.me/rw/` (read-write) |
| CI/CD | GitHub Actions: bun build → prerender → `wrangler pages deploy` |
| Email workers | `email-gateway-worker`, `email-to-slack-worker` |
| Cloudflare plan | $5/month Workers Paid |

## CI/CD Pipeline (`.github/workflows/deploy.yml`)

```
push to main → checkout (recursive submodules) → bun install --frozen-lockfile →
aws s3 cp from R2 → bun ./build-and-serve.ts → bun ./prerender.ts →
cp assets to dist/static/ → wrangler pages deploy
```

`build-and-serve.ts` builds then starts server so CSS hashes match between prerendered HTML and static assets.

## GitHub Secrets

| Secret | Value |
|--------|-------|
| `AWS_ACCESS_KEY_ID` | R2 read-only key |
| `AWS_SECRET_ACCESS_KEY` | R2 read-only secret |
| `AWS_ENDPOINT_URL` | R2 S3 endpoint |
| `AWS_BUCKET` | `briantakita-assets` |
| `ASSETS_BRIANTAKITA_ME__BUCKET_URL` | `https://briantakita.me` |
| `CLOUDFLARE_API_TOKEN` | Pages deploy token |

## Local Development

```bash
cd ~/work/btakita/briantakita.me-dev
mise trust && cd .. && cd briantakita.me-dev  # activate env
source .env
cd app/briantakita.me && bun run dev          # port 4100
```

## Esbuild Plugins

| Plugin | File | Purpose |
|--------|------|---------|
| `md_esbuild_plugin_` | `build.ts` | Imports raw `.md` files with YAML frontmatter as JS modules (auto-generates `meta_()` + `md__raw_()` exports) |
| `esmcss_esbuild_plugin_` | esmcss | CSS modules |
| `rebuild_tailwind_plugin_` | @rebuildjs/tailwindcss | Tailwind CSS |
| `preprocess_plugin_` | build.ts | Debug preprocessing for ctx-core |
| `json_esbuild_plugin_` | build.ts | JSON generation from `.json.ts` files |

## Blog Posts

**Published posts:** 51 total, including 4 new posts:
- `2026-03-05-introducing-agent-doc.md.ts` — "Introducing agent-doc" (TypeScript wrapper with rich content)
- `2026-03-12-dogfooding-agent-doc-part1-blog.md` — raw `.md` import via esbuild plugin
- `2026-03-12-dogfooding-agent-doc-part2-blog.md` — raw `.md` import
- `2026-03-12-dogfooding-agent-doc-part3-blog.md` — raw `.md` import

Post registration: `post/index.ts` — imports both `.md.js` (compiled from `.md.ts`) and `.md` (processed by `md_esbuild_plugin_`).

## Security Notes

- Google OAuth refresh tokens were exposed in March 13 session transcript — **revoke at https://myaccount.google.com/permissions**
- `CARGO_REGISTRY_TOKEN` was exposed in cleartext in March 14 session — **rotate at https://crates.io/settings/tokens**
- `/livestream` skill created at `.claude/skills/livestream/SKILL.md` for future sessions
- All future secret usage must use `$(pass ...)` — never inline cleartext

## Hono Migration (completed 2026-03-14)

Migrated from Elysia/relysjs to Hono/rhonojs:
- 10 app files: route handlers, build config, prerender
- 37 lib files across 7 submodules: imports `relysjs/server` → `rhonojs/server`
- 5 lib package.json: dep `relysjs` → `rhonojs`
- rhonojs 0.1.1 published with `app__attach()` fix for Hono sub-app routing

## Historical Fixes

| Issue | Fix |
|-------|-----|
| CSS hash mismatch on Pages | `build-and-serve.ts` builds+serves from same output |
| Slack webhook on wrong worker | Deleted stale root `wrangler.toml` |
| elysia→hono migration | rhonojs 0.1.1 with sub-app routing fix |
| prerender.ts relysjs import | Changed to `rhonojs/server/export` |
| bun.lock frozen lockfile CI failure | Regenerated `bun.lock` after dep changes |
| postcss circular import | Pinned `"postcss": "~8.4.49"` |

## Exchange

<!-- agent:exchange -->
### Re: request_url fix in rappstack-dev

Found your changes in `rappstack-dev/lib/domain--server/request/index.ts`:

1. **Null guard with `nullish__none_`** — `request_url__origin_` and `request_url__href_` now gracefully handle null `request_url` instead of crashing on destructuring
2. **Import fix** — `relysjs/server` → `rhonojs/server` (stale import from pre-migration)

This addresses the root cause: during prerender, `request_url_(ctx)` returns null because there's no real HTTP request. The `nullish__none_` wrapper short-circuits and returns `none` instead of throwing `TypeError: Cannot destructure property 'hostname' of null`.

**Next steps to get this into CI:**
1. Commit + push `domain--server` in rappstack-dev
2. Publish `@rappstack/domain--server` to npm (bump to 0.1.1)
3. In briantakita.me-dev: `bun update @rappstack/domain--server` to pick up the new version
4. Push briantakita.me-dev to trigger CI

Want me to handle the publish + integration, or are you driving this?

There are some type errors. Please fix those.
<!-- /agent:exchange -->
