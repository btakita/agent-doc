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

<!-- agent:exchange mode=append -->
### Re: Type errors fixed

Fixed `rappstack-dev/lib/domain--server/request/index.ts`:

- Simplified `request_url__href_` — replaced `nullish__none_(tup(...))` with explicit null checks. Clearer control flow, no `tup` import needed.
- Simplified `request_url__origin_` — replaced `nullish__none_([request_url_(ctx)], ...)` with explicit null guard. Added `request_ctx_T` type annotation to the callback parameter.
- Removed unused `tup` import.

Both functions now return `undefined` when inputs are nullish (during prerender), avoiding the `TypeError` crash.

**Type check result:** `request/index.ts` compiles cleanly. Two pre-existing errors remain in `jsonld/index.ts` and `rdfa/index.ts` (unrelated — `string | undefined` not assignable to `nested_url_segment_a_T`).

Ready for commit + publish when you want to proceed.

go

Please add automated tests, including unit tests, functional tests, smoke tests.

Avoid mocks if possible. Please add as a testing skill.

release

The *.md posts are slow to render.
Are we generating *.html files for **all** of the posts, including *.md source files?

Are all of the posts and YouTube videos in the sitemap?
Does the json-ld include the YouTube Videos?
See tasks/brookebrodack-dev.md to see recent run

The gh actions has numerous errors:
https://github.com/btakita/briantakita.me-dev/actions/runs/23202564958/job/67428798783

```
Run cd app/briantakita.me
[baseline-browser-mapping] The data in this module is over two months old.  To ensure accurate Baseline data, please update: `npm i baseline-browser-mapping@latest -D`
build|done
build|done
server started on port 4100
[static_export] using external server: http://localhost:4100
[static_export] site_url: ***
[static_export] discovered 158 routes from sitemap
[static_export] / -> dist/static/index.html (43703 bytes)
[static_export] /about -> dist/static/about/index.html (70210 bytes)
[static_export] /open-source -> dist/static/open-source/index.html (113927 bytes)
[static_export] /portfolio -> dist/static/portfolio/index.html (109185 bytes)
39 |     ] = ns_id_be_memo_pair_(
40 |       "request",
41 |       "request_url__origin",
42 |       (ctx) => {
43 |         const request_url = request_url_(ctx);
44 |         const { hostname, port } = request_url;
                                        ^
TypeError: Cannot destructure property 'hostname' from null or undefined value
      at <anonymous> (/home/runner/work/briantakita.me-dev/briantakita.me-dev/app/briantakita.me/dist/server--dev/index.server-PJ5X54UL.js:44:36)
      at memo__run (/home/runner/work/briantakita.me-dev/briantakita.me-dev/node_modules/ctx-core/all/rmemo/index.js:204:17)
      at memo (/home/runner/work/briantakita.me-dev/briantakita.me-dev/node_modules/ctx-core/all/rmemo/index.js:18:20)
      at <anonymous> (/home/runner/work/briantakita.me-dev/briantakita.me-dev/app/briantakita.me/dist/server--dev/index.server-PJ5X54UL.js:22:79)
      at memo__run (/home/runner/work/briantakita.me-dev/briantakita.me-dev/node_modules/ctx-core/all/rmemo/index.js:204:17)
      at memo (/home/runner/work/briantakita.me-dev/briantakita.me-dev/node_modules/ctx-core/all/rmemo/index.js:18:20)
      at <anonymous> (/home/runner/work/briantakita.me-dev/briantakita.me-dev/app/briantakita.me/dist/server--dev/index.server-PJ5X54UL.js:2287:12)
      at jsonld__add (/home/runner/work/briantakita.me-dev/briantakita.me-dev/app/briantakita.me/dist/server--dev/index.server-PJ5X54UL.js:246:17)
      at blog_post__main__article_ (/home/runner/work/briantakita.me-dev/briantakita.me-dev/app/briantakita.me/dist/server--dev/index.server-PJ5X54UL.js:2278:28)
      at blog_post__main_fragment_ (/home/runner/work/briantakita.me-dev/briantakita.me-dev/app/briantakita.me/dist/server--dev/index.server-PJ5X54UL.js:2235:7)
...
17 |   "../../node_modules/@rappstack/domain--server/request/index.ts"() {
18 |     [
19 |       ,
20 |       request_url__href_
21 |     ] = id_be_memo_pair_("request_url__href", (ctx) => {
22 |       return new URL(request_url__pathname_(ctx) + request_url__search_(ctx), request_url__origin_(ctx)).href;
                  ^
TypeError: "NaN" cannot be parsed as a URL.
 code: "ERR_INVALID_URL"

      at <anonymous> (/home/runner/work/briantakita.me-dev/briantakita.me-dev/app/briantakita.me/dist/server--dev/index.server-PJ5X54UL.js:22:14)
      at memo__run (/home/runner/work/briantakita.me-dev/briantakita.me-dev/node_modules/ctx-core/all/rmemo/index.js:204:17)
      at memo (/home/runner/work/briantakita.me-dev/briantakita.me-dev/node_modules/ctx-core/all/rmemo/index.js:18:20)
      at <anonymous> (/home/runner/work/briantakita.me-dev/briantakita.me-dev/app/briantakita.me/dist/server--dev/index.server-PJ5X54UL.js:2287:12)
      at jsonld__add (/home/runner/work/briantakita.me-dev/briantakita.me-dev/app/briantakita.me/dist/server--dev/index.server-PJ5X54UL.js:246:17)
      at blog_post__main__article_ (/home/runner/work/briantakita.me-dev/briantakita.me-dev/app/briantakita.me/dist/server--dev/index.server-PJ5X54UL.js:2278:28)
      at blog_post__main_fragment_ (/home/runner/work/briantakita.me-dev/briantakita.me-dev/app/briantakita.me/dist/server--dev/index.server-PJ5X54UL.js:2235:7)
      at post__doc_html_ (/home/runner/work/briantakita.me-dev/briantakita.me-dev/app/briantakita.me/dist/server--dev/index.server-PJ5X54UL.js:10288:5)

...
```

Please fix all of the errors.

### Re: Sitemap + JSON-LD for brookebrodack-dev

**Are all posts and YouTube videos in the sitemap?**

Yes for posts — all 190 blog posts are in the sitemap via `sorted_dehydrated_post_meta_a1_`. Posts with `video_url` get `<video:video>` metadata in the sitemap entry. All 190 posts have `video_url` fields, so all get video sitemap metadata.

Standalone YouTube videos (from the API, not associated with a blog post) are NOT in the sitemap — only blog post routes (`/content/{slug}`) appear.

**Does the JSON-LD include YouTube Videos?**

Yes, in two places:

1. **Individual post pages** (`post__doc_html.ts`) — when a post has `video_url`, a `VideoObject` is added with name, description, thumbnailUrl, contentUrl, embedUrl, uploadDate, and author reference.

2. **Content feed page** (`content__doc_html.ts`) — renders all YouTube API videos as `VideoObject` entries with `isPartOf` relationship to the main Article.

**Summary:** Blog posts with videos = in both sitemap AND JSON-LD. Standalone YouTube API videos = in JSON-LD on the content page only, NOT in sitemap.

Add automated tests for errors.
<!-- /agent:exchange -->
