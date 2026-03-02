---
session: ce60bbc6-783f-40ac-9639-4e37af731520
resume: null
agent: null
model: null
branch: null
---

Please look in ctx-core-dev. Note the layout. Some of the docs are incorrect.

Let's start development on it again.

## Assistant

**ctx-core-dev** — 187-package TypeScript monorepo (`lib/` + `tools/`) using pnpm workspaces + changesets. Core `ctx-core` package (v7.2.32) has 48 modular exports — zero-dependency, functional-first utilities.

**Current git state:**
- Branch: `main`, up-to-date with origin
- Staged: `.envrc` deleted, `.mise.toml` added (migrating direnv → mise)
- Unstaged: shell script quoting fixes in `bin/`

**Incorrect/outdated docs in `Readme.md`:**

1. **Lerna references** (lines 19-29) — repo uses pnpm workspaces, not Lerna
2. **Sapper mention** (line 46) — Sapper is dead, SvelteKit replaced it
3. **Quovo demo app** (line 101) — no longer exists in the repo
4. **Tech stack** (lines 42-47) — lists ES6/Svelte/Sapper, missing Solid.js, auth packages, and modern tooling

**No CLAUDE.md** exists yet for this project.

Where would you like to start — doc fixes, committing the staged changes, or something else?

## User

Doc fixes. Let's add a AGENTS.md and CLAUDE.md symlink.
There was some automation tools in managing the updates and upgrades.

changeset-version-COMMIT_EDITMSG
recursive-git-commit
pnpm-publish--force

## Assistant

Created `AGENTS.md` and `CLAUDE.md → AGENTS.md` symlink in ctx-core-dev.

**AGENTS.md covers:**
- Architecture overview (lib/, tools/, bin/, dormant/)
- Stack (TypeScript ES2022, pnpm workspaces, changesets, uvu, mise)
- Core package (`ctx-core` v7.2.32, 48 exports)
- **Release workflow** — the three automation tools you mentioned:
  1. `changeset-version-COMMIT_EDITMSG` — runs `pnpm changeset version`, generates CHANGELOG diffs, populates per-package COMMIT_EDITMSG files
  2. `recursive-git-commit` — opens editor for review, then commits recursively in each workspace package using COMMIT_EDITMSG
  3. `pnpm-publish--force` — parallel publish via GNU `parallel`, skips already-published versions, supports `-d` dry run
- Dev environment commands (`tmux-dev`, `build-watch`, etc.)
- Conventions and git rules

All three scripts live in `lib/monorepo/bin/` and are exposed via PATH in `.mise.toml`.

Still to do: the `Readme.md` has 4 outdated sections (Lerna, Sapper, Quovo, tech stack). Want me to fix those next?

## User

Please fix docs.
