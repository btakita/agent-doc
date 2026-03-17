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
### Re: Phase 1-5 complete

**Phase 1: npm verification** — All 11 `@rappstack/*` packages confirmed published on npm:

| Package | npm version |
|---------|-------------|
| @rappstack/domain--any--blog | 0.1.2 |
| @rappstack/domain--browser | 0.1.0 |
| @rappstack/domain--browser--blog | 0.1.0 |
| @rappstack/domain--server | 0.1.0 |
| @rappstack/domain--server--blog | 0.1.1 |
| @rappstack/ui--any | 0.1.0 |
| @rappstack/ui--any--blog | 0.1.0 |
| @rappstack/ui--browser | 0.1.1 |
| @rappstack/ui--browser--blog | 0.1.0 |
| @rappstack/ui--server | 0.1.1 |
| @rappstack/ui--server--blog | 0.1.1 |

**Phase 2: Submodule removal** — All 11 rappstack submodules removed (`git submodule deinit` + `git rm` + `.git/modules` cleanup).

**Phase 3: Dependency updates** — Changed `workspace:*` → version ranges in 5 files:
- `app/briantakita.me/package.json` (9 deps)
- `lib/domain--server--briantakita/package.json` (2 deps)
- `lib/ui--browser--briantakita/package.json` (1 dep)
- `lib/ui--server--briantakita/package.json` (6 deps)
- `lib/snippet--rappstack/package.json` (3 deps)

**Phase 4: Workspace config** — No change needed. `"lib/*"` glob still matches remaining 7 btakita packages.

**Phase 5: yalc setup** — `.yalc/` and `yalc.lock` already in `.gitignore`.

**Verification:**
- `bun install` — resolved all deps from npm registry (13 installed, 11 workspace links removed)
- `bun run build` — builds successfully

**Remaining:**
- Phase 6: CI verification (push and test GitHub Actions)
- Phase 7: Documentation
- Commit submodule changes from within each modified btakita lib (4 submodules have package.json changes)

Note: The btakita lib package.json changes need to be committed from within each submodule repo, then pushed, before committing the parent pointer updates. Want me to proceed with the submodule commits?

yes
<!-- /agent:exchange -->
