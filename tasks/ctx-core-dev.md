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
