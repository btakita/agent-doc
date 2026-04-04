---
agent_doc_session: ce60bbc6-783f-40ac-9639-4e37af731520
tmux_session: '0'
---


## Status Summary (compacted 2026-03-06)

### ctx-core-dev — 192-package TypeScript Monorepo

**Path:** `/home/brian/work/ctx-core/ctx-core-dev`
**Stack:** TypeScript `next` (6.0-dev), bun workspaces (migrated from pnpm), changesets, uvu, mise
**Core:** `ctx-core` v7.2.34 — 48 modular exports, zero-dependency

### Documentation

- **AGENTS.md** (+ CLAUDE.md symlink) — architecture, release workflow, dependency management, naming convention
- **NAMING.md** — Tag Path naming system (universal meta-convention for immutable tag composition)

### Naming System: Tag Path

Crate: `tagpath` — repo at https://github.com/btakita/tagpath (submodule in agent-loop)
- Parses identifiers across conventions (snake_case, camelCase, PascalCase, kebab-case)
- Canonical tags as lingua franca for cross-convention semantic transfer
- 12 tests passing, Phase 1 complete (parse, init, config)

### Release Workflow (updated for bun)

```bash
ctx_core__monorepo_pnpm__dependencies__update   # query npm, update deps, create changesets
bun install                                      # install
changeset-version-COMMIT_EDITMSG                 # version + CHANGELOG diffs
recursive-git-commit                             # commit per package (incl. dormant submodules)
publish--force                                   # parallel publish via bun (needs npm granular token with 2FA bypass)
git push --recurse-submodules=on-demand          # push all (submodules first, then parent)
```

### All 192 Packages Published (2026-03-05)

**Changes applied monorepo-wide:**
- Replaced `check-dts` with `tsc --noEmit` in 184 packages (check-dts hardcodes `moduleResolution: node10`, incompatible with TS 6)
- Changed `module: ESNext` → `module: NodeNext` in 186+ tsconfigs (TS 6 requires module + moduleResolution match)
- Removed `check-dts` from devDependencies
- Added `types: ["node"]` to 10 tsconfigs
- Fixed import extensions, JSON import attributes, svelte 5 store compatibility
- Excluded `_fixtures/` from tsconfig in rebuildjs + @rebuildjs/tailwindcss

**auth0 v5 migration:** Updated `@ctx-core/auth0-management` — 7 files migrated from `UserProfile`/`Client`/`ClientGrant`/`UserUpdate` to `Management.*` types

### pnpm → bun Migration (2026-03-06)

**Completed — all committed and pushed to GitHub.**

**Root repo changes:**
| File | Change |
|------|--------|
| `AGENTS.md` | Package manager → bun, yalc docs, updated release workflow |
| `bin/bun-update.sh` | Renamed from `pnpm-up.sh`, runs `bun update && strip-workspace.sh` |
| `bin/yalc-publish-all.sh` | New — publishes all 190 workspace packages to yalc local store |
| `package.json` | Workspaces as array, overrides moved from `pnpm.overrides`, removed pnpm deps, added yalc + publint |
| `bun.lock` | New lockfile (replaced `pnpm-lock.yaml`) |

**Submodule changes (193 packages):**
| Change | Scope |
|--------|-------|
| `prepublishOnly`: `pnpm` → `bun`, added `publint && attw --pack --ignore-rules cjs-resolves-to-esm` | 186 packages |
| `test`: expanded `bun run /^test:/` regex → explicit `bun run test:types && bun run test:unit` chains | 182 packages |
| `lib/monorepo` — 15 scripts migrated, `publish--force` renamed from `pnpm-publish--force`, `workspace-list.sh` + `workspace-exec.sh` added, `dirname $0` → `readlink -f` for symlink resolution | 1 package |
| `lib/build` — build_watch_cli reads package.json workspaces, uses `bun run build` | 1 package |
| `lib/pnpm-tools` — reads package.json instead of pnpm-workspace.yaml (dead code, deprecated on npm) | 1 package |

**Issues fixed during migration:**
1. `bun install` deadlock — pnpm lockfile migration hung; deleted pnpm-lock.yaml and let bun resolve fresh
2. `bun run /^test:/` — bun doesn't support pnpm's regex script matching; expanded to explicit `&&` chains
3. `attw --pack` returns non-zero for `CJSResolvesToESM` on ESM-only packages; added `--ignore-rules`
4. `dirname $0` in monorepo scripts broke when invoked via `node_modules/.bin/` symlinks; fixed with `readlink -f`
5. `publint` flagged `sort-packages.mjs` shebang (`#!/bin/env` → `#!/usr/bin/env`)

**Version bumps:** Patch bump created via changesets for all 190 packages. Committed and pushed but NOT yet published to npm. The version bump was premature — will be published in the next regular release cycle.

**Published:** `@ctx-core/monorepo@21.4.161` — includes `publish--force` rename + publint fixes.

**Deprecated:** `@ctx-core/pnpm-tools` on npm → "use @ctx-core/monorepo instead"

### yalc for Cross-Monorepo Development

**Problem:** Symlinked packages across monorepos cause TypeScript type identity conflicts — same package resolved from two different `node_modules` paths creates incompatible types (e.g., postcss Plugin type from `.bun/postcss@8.5.8/...` vs `briantakita.me-dev/node_modules/postcss/...`).

**Solution:** yalc copies packages instead of symlinking. Each consumer gets its own resolution tree.

**Setup:**
```bash
# In ctx-core-dev (after changes):
yalc-publish-all.sh          # --push is default, publishes 190 packages

# In consumer (one-time):
yalc add ctx-core rebuildjs relysjs relementjs rmemo esmcss \
  @ctx-core/dev-tools @ctx-core/monorepo @ctx-core/preprocess \
  @ctx-core/source-map @rebuildjs/tailwindcss
bun install

# To revert:
yalc remove --all && bun install
```

**briantakita.me-dev:** postinstall hook (`ctx-core--link.sh`) already removed. yalc packages added. The `fdir` resolution error is fixed.

### Open Items

- **ASSETS_BRIANTAKITA_ME__BUCKET_URL**: Only used in one blog post (`2024-03-05-squarespace-site-lighthouse-scores.md.ts`) for lighthouse iframes. User moved hosting to Cloudflare — env var may need updating or removing. This is a briantakita.me-dev issue.
- **Function names containing "pnpm"** (e.g., `monorepo_pnpm__dependencies__update`) left unchanged to avoid API breakage — rename in a future refactor.
- **Full npm publish** of the 190 package patch bumps — deferred to next regular release cycle.

## User
