---
agent_doc_session: ce60bbc6-783f-40ac-9639-4e37af731520
tmux_session: '1'
---

## Status Summary (compacted 2026-03-05)

### ctx-core-dev — 192-package TypeScript Monorepo

**Path:** `/home/brian/work/ctx-core/ctx-core-dev`
**Stack:** TypeScript `next` (6.0-dev), pnpm workspaces, changesets, uvu, mise
**Core:** `ctx-core` v7.2.33 — 48 modular exports, zero-dependency

### Documentation

- **AGENTS.md** (+ CLAUDE.md symlink) — architecture, release workflow, dependency management, naming convention
- **NAMING.md** — Tag Path naming system (universal meta-convention for immutable tag composition)

### Naming System: Tag Path

Crate: `tagpath` — repo at https://github.com/btakita/tagpath (submodule in agent-loop)
- Parses identifiers across conventions (snake_case, camelCase, PascalCase, kebab-case)
- Canonical tags as lingua franca for cross-convention semantic transfer
- 12 tests passing, Phase 1 complete (parse, init, config)

### Release Workflow

```bash
ctx_core__monorepo_pnpm__dependencies__update   # query npm, update deps, create changesets
pnpm i --recursive                               # install
changeset-version-COMMIT_EDITMSG                 # version + CHANGELOG diffs
recursive-git-commit                             # commit per package (incl. dormant submodules)
pnpm-publish--force                              # parallel publish (needs npm granular token with 2FA bypass)
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

**`recursive-git-commit` updated:** Now also commits in git submodules (dormant/) via `git submodule foreach`

**Key published packages:** rebuildjs@0.70.44, relysjs@1.21.74, @rebuildjs/tailwindcss@0.10.59, ctx-core@7.2.33

### Fixes Applied

| Issue | Fix |
|-------|-----|
| `update-npm-packages` broken | Delegates to `monorepo_npm__dependencies__update.mjs` |
| Missing `cli-cursor` (ora dep) | `pnpm install --recursive` |
| `cd: null directory` warnings | Guard in `CHANGELOG-diff-COMMIT_EDITMSG.sh` |
| pnpm install fails after changeset-version | `link-workspace-packages=true` in `.npmrc` |
| `check-dts` + TS 6 `node10` error | Replaced with `tsc --noEmit` monorepo-wide |
| auth0 v5 breaking types | Migrated to `Management.*` type namespace |
| npm auth expired / 2FA required | Granular access token with 2FA bypass |
| dormant submodules not committed | `git submodule foreach` added to `recursive-git-commit.sh` |

### Pending

- esbuild bumped to `^0.27.3` in 4 packages (rebuildjs-tailwindcss, rebuildjs, esbuild-plugin-esmfile, esbuild-plugin-esmcss)
- tailwindcss pinned to `^3.4.18` (v4 migration deferred)
- `npm warn Unknown project config "link-workspace-packages"` — harmless, pnpm setting ignored by npm

## User

How do i perform a recursive git push?

How do I ensure that the root repo does not orphan any submodule version by not pushing the submodule to the origin?
