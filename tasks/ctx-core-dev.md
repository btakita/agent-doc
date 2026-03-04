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

