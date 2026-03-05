---
agent_doc_session: 7322672d-726a-47c7-9a81-a836b968bdc9
tmux_session: '1'
---

## Status Summary (compacted 2026-03-04)

### briantakita.me-dev — Local Development Setup

**Symlink:** `briantakita.me-dev -> /home/brian/work/btakita/briantakita.me-dev`

**Dev workflow:**
```bash
mise trust && cd .. && cd briantakita.me-dev  # activate env
export AWS_ACCESS_KEY_ID=$(pass briantakita.me/AWS_ACCESS_KEY_ID)
export AWS_SECRET_ACCESS_KEY=$(pass briantakita.me/AWS_SECRET_ACCESS_KEY)
assets--download--sync                         # sync S3 assets to public/
bun run dev                                    # build + start on port 4100
```

### ctx-core-dev Symlinking (17 packages)

`ctx-core--link` (runs via `postinstall`) symlinks local ctx-core-dev packages:
- 9 @ctx-core/* packages (dev-tools, monorepo, preprocess, source-map, child_process, ctx-core-package-tools, dir, fs, package)
- @rebuildjs/tailwindcss, esbuild-plugin-esmfile, esbuild-plugin-esmcss
- ctx-core, relysjs, rebuildjs, relementjs, rmemo

**Dedup step:** Removes nested esbuild/postcss from symlinked packages to prevent TypeScript TS2322 (pnpm store vs bun — same version at different paths = incompatible types).

Scripts: `app/-briantakita.me-dev-bin/bin/ctx-core--link.sh`, `ctx-core--unlink.sh`

### Fixes Applied

| Issue | Fix |
|-------|-----|
| S3 401 Unauthorized | `.env` needs `source` or mise — `$(pass ...)` not expanded by dotenv |
| elysia mapResponse removed | Replaced with inline Response conversion in relysjs compression |
| postcss@8.5.6 circular import | Pinned `"postcss": "~8.4.49"` in root overrides |
| esbuild TS2322 type mismatch | Updated ctx-core-dev esbuild `^0.23.1` → `^0.27.3` in 4 packages; dedup nested copies in link script |
| tailwindcss v4 PostCSS breakage | Pinned `"tailwindcss": "^3.4.18"` in @rebuildjs/tailwindcss |
| baseline-browser-mapping warning | v2.10.0 is latest; data stale from Aug 2023. Suppress: `BROWSERSLIST_IGNORE_OLD_DATA=true` |
| TimeoutOverflowWarning: Infinity | rmemo's `rmemo__wait` passes Infinity to setTimeout — harmless, cosmetic |
| Bun bin symlinks not created | Manual `ln -sf` workaround; `bun install --force` doesn't always regenerate bin entries |
| Broken workspace symlinks after rm -rf | link script cleans stale symlinks before relinking |

### Pending

- `ASSETS_BRIANTAKITA_ME__BUCKET_URL|missing` — mise `.env` not loading in current shell
- Tailwind v4 migration for @rebuildjs/tailwindcss (v3 pinned for now)
- `ctx-core--link.sh` added `.npmrc` with `link-workspace-packages=true` to ctx-core-dev

## User

Please update all libraries in briantakita.me-dev, run tests, and commit/push when working.
