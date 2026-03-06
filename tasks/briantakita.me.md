---
agent_doc_session: 7322672d-726a-47c7-9a81-a836b968bdc9
tmux_session: '0'
---


## Status Summary (compacted 2026-03-05)

### Current Architecture

**briantakita.me** is a pre-rendered static site deployed to **Cloudflare Pages**.

| Component | Details |
|-----------|---------|
| Hosting | Cloudflare Pages (`briantakita-me` project) |
| DNS | `briantakita.me` → Cloudflare Pages (nameservers: `dion.ns.cloudflare.com`, `tegan.ns.cloudflare.com`) |
| Assets | R2 bucket `briantakita-assets` (95 objects, ~31MB) |
| Asset URL | `ASSETS_BRIANTAKITA_ME__BUCKET_URL=https://briantakita.me` |
| R2 endpoint | `https://1e7e194ff8da4ad9e40ce041aea84f76.r2.cloudflarestorage.com` |
| R2 credentials | `pass briantakita.me/r/` (read), `pass briantakita.me/rw/` (read-write) |
| CI/CD | GitHub Actions: bun build → prerender → `wrangler pages deploy` |
| Email workers | `email-gateway-worker`, `email-to-slack-worker` (SLACK_WEBHOOK_URL set) |
| Cloudflare plan | $5/month Workers Paid (covers Pages, R2, Workers) |

### CI/CD Pipeline (`.github/workflows/deploy.yml`)

```
push to main → checkout (recursive submodules) → bun install →
aws s3 cp from R2 → bun ./build-and-serve.ts → bun ./prerender.ts →
cp assets to dist/static/ → wrangler pages deploy
```

Key detail: `build-and-serve.ts` builds then starts server from same output so CSS hashes match between prerendered HTML and static assets.

### GitHub Secrets (briantakita.me-dev)

| Secret | Value |
|--------|-------|
| `AWS_ACCESS_KEY_ID` | R2 read-only key |
| `AWS_SECRET_ACCESS_KEY` | R2 read-only secret |
| `AWS_ENDPOINT_URL` | R2 S3 endpoint |
| `AWS_BUCKET` | `briantakita-assets` |
| `ASSETS_BRIANTAKITA_ME__BUCKET_URL` | `https://briantakita.me` |
| `CLOUDFLARE_API_TOKEN` | Pages deploy token |
| `VPS_HOST`, `VPS_USER`, `VPS_SSH_KEY` | Stale (can be removed) |

### Local Development

```bash
# In ~/work/btakita/briantakita.me-dev
mise trust && cd .. && cd briantakita.me-dev  # activate env
source .env
cd app/briantakita.me && bun run dev          # port 4100
```

`.env` keys: `AWS_REGION=auto`, `AWS_ENDPOINT_URL`, `AWS_BUCKET=briantakita-assets`, `ASSETS_BRIANTAKITA_ME__BUCKET_URL=https://briantakita.me`

### ctx-core-dev Symlinking (17 packages)

`ctx-core--link` (postinstall) symlinks local ctx-core-dev packages into `node_modules/`. Local uses unreleased versions; CI uses published npm versions.

### Cloudflare Workers Submodule

`cloudflare/` submodule (`git@github.com:btakita/cloudflare.git`), symlinked at `~/work/btakita/cloudflare`.

Workers have individual `wrangler.toml` in subdirectories — no root `wrangler.toml` (deleted stale one that caused `rapid-brook-7c04` misrouting).

### Contabo VPS (partially decommissioned)

- IP: `207.244.234.186`, SSH: `ssh contabo`
- `briantakita-www-1` container stopped; 8 other containers still running
- Contabo S3 bucket `assets.briantakita.me` can be deleted (fully mirrored to R2)

### Historical Fixes

| Issue | Fix |
|-------|-----|
| CSS hash mismatch on Pages | Created `build-and-serve.ts` to build+serve from same output |
| Lighthouse HTML 404s on Pages | Added `cp -r ../../public/assets/* dist/static/` to CI |
| Slack webhook on wrong worker | Deleted stale root `wrangler.toml`, set secret on correct worker |
| R2 pass entry wrong value | Fixed `briantakita.me/r/AWS_ACCESS_KEY_ID` in pass |
| elysia mapResponse removed | Updated relysjs 1.21.73→1.21.74 |
| postcss@8.5.6 circular import | Pinned `"postcss": "~8.4.49"` |
| tailwindcss v4 PostCSS breakage | Pinned `"tailwindcss": "^3.4.18"` |

### Future Considerations

- **Hono migration**: Not needed now (site is static), but planned for Judith's Equity Funding Source app. Hono is portable (Workers, Deno, Bun, Node) — no lock-in.
- **Tailwind v4**: Pinned to v3 for now
- **Workers SSR**: If dynamic endpoints needed later, add Hono Worker alongside static Pages

### Blog Post

`app/briantakita.me/post/content/2026-03-05-introducing-agent-doc.md.ts`

### Pending

- [ ] Verify email-to-Slack works (send test to `dev-slack@briantakita.me`)
- [ ] Remove stale GitHub secrets (`VPS_HOST`, `VPS_USER`, `VPS_SSH_KEY`)
- [ ] Delete Contabo S3 bucket `assets.briantakita.me`

## User

Does briantakita.me-dev repos have yalc dependencies to all packages in ctx-core-dev?

## Assistant

**No — only the 11 packages it actually uses**, not all 190:

```
@ctx-core/dev-tools    @ctx-core/monorepo     @ctx-core/preprocess
@ctx-core/source-map   @rebuildjs/tailwindcss  ctx-core
esmcss                 rebuildjs              relementjs
relysjs                rmemo
```

These match exactly the deps declared in briantakita.me-dev's workspace package.json files. `yalc-publish-all.sh` publishes all 190 to the yalc store, but only the ones you `yalc add` in the consumer are linked. If you add a new ctx-core dependency to briantakita.me-dev later, you'd need to `yalc add <pkg>` for it too.

## User

Please run yalc add on the dependencies. If not already added.
