---
agent_doc_session: ce60bbc6-783f-40ac-9639-4e37af731520
tmux_session: '1'
---

## Status Summary (compacted 2026-03-04)

### ctx-core-dev — 187-package TypeScript Monorepo

**Path:** `/home/brian/work/ctx-core/ctx-core-dev`
**Stack:** TypeScript ES2022, pnpm workspaces, changesets, uvu, mise
**Core:** `ctx-core` v7.2.32 — 48 modular exports, zero-dependency

### Documentation

- **AGENTS.md** (+ CLAUDE.md symlink) — architecture, release workflow, dependency management, naming convention
- **NAMING.md** — Tag Path naming system (universal meta-convention for immutable tag composition)
- **Readme.md** — fixed: Lerna → pnpm, removed Sapper/Quovo, updated tech stack

### Naming System: Tag Path

Crate: `tagpath` — repo at https://github.com/btakita/tagpath (submodule in agent-loop)
- Parses identifiers across conventions (snake_case, camelCase, PascalCase, kebab-case)
- Canonical tags as lingua franca for cross-convention semantic transfer
- `.naming.yml` config with layers: Convention, Tags, Vectors, Patterns, Externals, Packages
- English prose as a convention variant (bidirectional translation)
- 12 tests passing, Phase 1 complete (parse, init, config)

### Release Workflow

```bash
ctx_core__monorepo_pnpm__dependencies__update   # query npm, update deps, create changesets
pnpm i --recursive                               # install
changeset-version-COMMIT_EDITMSG                 # version + CHANGELOG diffs
recursive-git-commit                             # commit per package
pnpm-publish--force                              # parallel publish
```

### Fixes Applied

| Issue | Fix |
|-------|-----|
| `update-npm-packages` broken | Now delegates to `monorepo_npm__dependencies__update.mjs` |
| Missing `cli-cursor` (ora dep) | `pnpm install --recursive` — installed 11 missing packages |
| `cd: null directory` warnings | Guard added in `CHANGELOG-diff-COMMIT_EDITMSG.sh` to skip blank lines |
| pnpm install fails after changeset-version | Added `link-workspace-packages=true` to `.npmrc` |

### Pending

- esbuild bumped to `^0.27.3` in 4 packages (rebuildjs-tailwindcss, rebuildjs, esbuild-plugin-esmfile, esbuild-plugin-esmcss)
- tailwindcss pinned to `^3.4.18` (v4 migration deferred)
- Changeset versions bumped but not yet published — need `pnpm-publish--force`
- Unstaged: shell script quoting fixes in `bin/`, `.mise.toml` addition

## User

npm warn Unknown project config "link-workspace-packages". This will stop working in the next major version of npm.

## Assistant

Harmless warning. `link-workspace-packages` is a pnpm-specific `.npmrc` setting — npm doesn't recognize it but ignores it and continues. Since ctx-core-dev uses pnpm exclusively, pnpm reads the setting correctly.

The warning likely came from an `npm` or `npx` invocation (e.g., a script internally calling npm). No action needed — pnpm handles it fine.

## User

> @ctx-core/carousel@11.2.444 test /home/brian/work/ctx-core/ctx-core-dev/lib/carousel
> pnpm run /^test:/

. test:types$ check-dts
. test:unit$ tsx node_modules/uvu/bin.js . '\.test\.(ts|js)$'
. test:unit:   Total:     0
. test:unit:   Passed:    0
. test:unit:   Skipped:   0
. test:unit:   Duration:  4.77ms
. test:unit: Done
. test:types: - Check types
. test:types: ✖ Check types
. test:types: Option 'moduleResolution=node10' is deprecated and will stop functioning in TypeScript 7.0. Specify compilerOption '"ignoreDeprecations": "6.0"' to silence this error.
. test:types: Failed
 ELIFECYCLE  Command failed with exit code 1.
 ELIFECYCLE  Test failed. See above for more details.
 ELIFECYCLE  Command failed with exit code 1.
@ctx-core/carousel NOT PUBLISHED: Publish failed
npm warn Unknown project config "link-workspace-packages". This will stop working in the next major version of npm.
npm warn Ignoring workspaces for specified package(s)
ETA: 62s Left: 159 AVG: 0.45s  local:32/33/100%/0.5s
> esbuild-plugin-esmfile@0.2.11 prepublishOnly /home/brian/work/ctx-core/ctx-core-dev/lib/esbuild-plugin-esmfile
> pnpm clean && pnpm build && pnpm test


> esbuild-plugin-esmfile@0.2.11 clean /home/brian/work/ctx-core/ctx-core-dev/lib/esbuild-plugin-esmfile
> :


> esbuild-plugin-esmfile@0.2.11 build /home/brian/work/ctx-core/ctx-core-dev/lib/esbuild-plugin-esmfile
> :


> esbuild-plugin-esmfile@0.2.11 test /home/brian/work/ctx-core/ctx-core-dev/lib/esbuild-plugin-esmfile
> pnpm run /^test:/

. test:types$ check-dts
. test:unit$ tsx node_modules/uvu/bin.js . '\.test\.(ts|js)$'
. test:types: - Check types
. test:unit: Error [ERR_MODULE_NOT_FOUND]: Cannot find package 'esbuild' imported from /home/brian/work/ctx-core/ctx-core-dev/lib/esbuild-plugin-esmfile/index.test.ts
. test:unit: Did you mean to import "esbuild/lib/main.js"?
. test:unit:     at Object.getPackageJSONURL (node:internal/modules/package_json_reader:316:9)
. test:unit:     at packageResolve (node:internal/modules/esm/resolve:768:81)
. test:unit:     at moduleResolve (node:internal/modules/esm/resolve:858:18)
. test:unit:     at defaultResolve (node:internal/modules/esm/resolve:990:11)
. test:unit:     at nextResolve (node:internal/modules/esm/hooks:769:28)
. test:unit:     at resolveBase (file:///home/brian/work/ctx-core/ctx-core-dev/node_modules/.pnpm/tsx@4.21.0/node_modules/tsx/dist/esm/index.mjs?1772661817989:2:3744)
. test:unit:     at resolveDirectory (file:///home/brian/work/ctx-core/ctx-core-dev/node_modules/.pnpm/tsx@4.21.0/node_modules/tsx/dist/esm/index.mjs?1772661817989:2:4243)
. test:unit:     at resolveTsPaths (file:///home/brian/work/ctx-core/ctx-core-dev/node_modules/.pnpm/tsx@4.21.0/node_modules/tsx/dist/esm/index.mjs?1772661817989:2:4984)
. test:unit:     at resolve (file:///home/brian/work/ctx-core/ctx-core-dev/node_modules/.pnpm/tsx@4.21.0/node_modules/tsx/dist/esm/index.mjs?1772661817989:2:5361)
. test:unit:     at nextResolve (node:internal/modules/esm/hooks:769:28)
. test:unit: Failed
 ELIFECYCLE  Command failed with exit code 1.
 ELIFECYCLE  Test failed. See above for more details.
 ELIFECYCLE  Command failed with exit code 1.
esbuild-plugin-esmfile NOT PUBLISHED: Publish failed
npm warn Unknown project config "link-workspace-packages". This will stop working in the next major version of npm.
npm warn Ignoring workspaces for specified package(s)
ETA: 77s Left: 158 AVG: 0.53s  local:32/34/100%/0.5s
> @ctx-core/color-bar@14.2.219 prepublishOnly /home/brian/work/ctx-core/ctx-core-dev/lib/color-bar
> pnpm clean && pnpm build && pnpm test


> @ctx-core/color-bar@14.2.219 clean /home/brian/work/ctx-core/ctx-core-dev/lib/color-bar
> :


> @ctx-core/color-bar@14.2.219 build /home/brian/work/ctx-core/ctx-core-dev/lib/color-bar
> :
