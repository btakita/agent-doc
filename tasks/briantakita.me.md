---
agent_doc_session: 7322672d-726a-47c7-9a81-a836b968bdc9
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
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

<!-- agent:exchange mode=append -->
### Session Summary (compacted)

**Prior sessions resolved (9 items):** CI TypeError, esbuild 0.27 `.ts` resolution, Hono sub-app routing, video sitemap, search icon CSS, 32 tests, rebuildjs@0.71.1, local dev symlink workflow, @rappstack/ui--browser--blog@0.1.2 release.

**This session completed (6 items):**
1. **Scroll-responsive header** — JS scroll handler toggles `scrolled` class on `<html>`, CSS rules compact header/breadcrumb/sticky titles when scrolled
2. **Static size reductions** — Logo `w-12 h-7`, sticky offsets reduced
3. **Sticky alignment** — Breadcrumb margin removed, HR wrapper hidden, all sticky `top` values flush-stacked
4. **Transition glitch fix** — Removed CSS transitions (caused arrow-key scroll jumps)
5. **Blog post** — `2026-03-18-agent-doc-tmux-crdt-live.md` added and deployed
6. **Deployed** — All pushed to 4 submodules, CI passed, live on `briantakita.me`

**Open items:**
- JSON-LD VideoObject for video posts
- `@source` absolute path won't work on CI
- Search icon `w-full` class may need removal
- Scroll-responsive behavior may need browser cache clear on production
- `rappstack-dev` `blog__header.ts` changes are separate from briantakita.me-dev
continue
### Re: continue

Acknowledged the compacted exchange. The open items from prior sessions are:

1. **JSON-LD VideoObject** for video posts (not yet implemented)
2. **`@source` absolute path** won't work on CI — needs safelist or conditional include
3. **Search icon `w-full` class** may need removal
4. **Scroll-responsive behavior** may need browser cache clear on production
5. **`rappstack-dev` `blog__header.ts`** changes are separate from briantakita.me-dev

Which would you like to tackle, or is there new work to start?
1 and 2
### Re: 1 and 2

**1. JSON-LD VideoObject — already done.** `post__doc_html.ts` already injects `VideoObject` structured data when `video_url` is present in frontmatter. All 13 video posts have it. Sitemap `<video:video>` entries are also generated. No work needed — closing this item.

**2. `@source` absolute path fix.** The problem is in `app/briantakita.me/app/index.css.ts`:
\`\`\`css
@source "/home/brian/work/btakita/agent-loop/src/rappstack-dev/lib/**/*.ts";
\`\`\`
This hardcoded path won't exist on CI (GitHub Actions uses `/home/runner/work/...`). Options:

| Option | Approach | Pros | Cons |
|--------|----------|------|------|
| **A. Safelist** | List rappstack Tailwind classes explicitly | Fast builds, no scanning, works everywhere | Manual maintenance |
| **B. Env var** | `RAPPSTACK_DEV_ROOT` build-time var | Flexible, works anywhere | Build complexity |
| **C. Symlink** | `ln -s` rappstack-dev into project tree | Portable, simple relative path | Git + CI symlink setup |
| **D. Fix esmcss** | Resolve `@source` relative to CSS file, not temp dir | Root cause fix | Deeper change |

Which approach do you prefer?
### Re: Frontmatter updated

Noted the frontmatter changes: removed `tmux_session: '0'`, added `claude_args: --dangerously-skip-permissions`. The `@source` absolute path fix is still pending your approach decision (A/B/C/D).
<!-- agent:boundary:b54487e7 -->
<!-- /agent:exchange -->

<!-- agent:pending -->
- `@source` absolute path CI fix — awaiting approach decision (A/B/C/D)
<!-- /agent:pending -->
