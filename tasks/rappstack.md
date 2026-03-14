---
agent_doc_session: fce94c2a-2072-49c6-924b-144e96b540b7
tmux_session: '0'
agent_doc_mode: template
---


The @rappstack/* packages were recently published.
The apps used submodules from rappstack.
Now that @rappstack/* packages are published, the apps need to reference the @rappstack/* packages instead of using submodules.

Convert brookebrodack-dev, cogov-dev, censible-core, drmanny-dev, herbaliciousbliss-dev, prescience-notes-dev

Use yalc if possible. And deploy via github actions. The deployments must depend on the published npm packages...not yalc. yalc is only for development.

<!-- agent:exchange -->
## Conversion Plan

**Apps and their @rappstack/* dependencies:**

| App | @rappstack/* packages | Count |
|-----|----------------------|-------|
| herbaliciousbliss-dev | domain--any--blog, domain--server, domain--server--blog, ui--any, ui--any--blog, ui--browser--blog, ui--server, ui--server--blog | 8 |
| drmanny-dev | domain--any--blog, domain--server, domain--server--auth, domain--server--blog, ui--any, ui--server | 6 |
| brookebrodack-dev | domain--server, domain--server--auth, ui--any, ui--server | 4 |
| prescience-notes-dev | domain--server, domain--server--blog, ui--any, ui--browser--blog | 4 |
| cogov-dev | domain--server | 1 |
| censible-core | (none found — may not need conversion) | 0 |

**Conversion steps per app:**
1. Change `workspace:*` → published version in all package.json files
2. Remove rappstack git submodules from `lib/`
3. Remove submodule entries from `.gitmodules`
4. Remove workspace entries from `pnpm-workspace.yaml` (if applicable)
I thought we were using bun to manage the packages and deployments. Should pnpm-workspace.yaml be removed in favor of the bun monorepo configuration?
5. Run `pnpm install` to pull from npm registry
6. Verify build

Ready to start. Which app first, or all sequentially?
All in sequence.
### Re: bun vs pnpm & sequencing

Good catch — all apps already use **bun** (bun.lockb present, no pnpm-workspace.yaml). The corrected steps:

1. Change `workspace:*` → published version in package.json
2. Remove rappstack git submodules from `lib/`
3. Remove submodule entries from `.gitmodules`
4. Remove rappstack entries from `workspaces` in root package.json (keep app-specific libs like `domain--server--brookebrodack`)
5. Run `bun install` to pull from npm registry
6. Verify build

**Blocker: @rappstack packages are NOT on npm yet.**

All `npm view @rappstack/*` commands return 404. The packages need to be published first. The versions in rappstack-dev are all `0.0.0`.

**Before conversion, we need to:**
1. Set proper versions in rappstack-dev (e.g., `0.1.0`)
2. Publish all @rappstack/* packages to npm
3. Then proceed with app conversion

Should I publish the rappstack packages first? Or are they published somewhere else (private registry, GitHub Packages)?

Publish rappstack packages first.
### Re: Publishing rappstack packages

**Two blockers before we can publish:**

**1. @rappstack npm org doesn't exist yet**
The `@rappstack` scope needs to be created on npm. You need to:
- Go to https://www.npmjs.com/org/create
- Create the `rappstack` org (your `btakita` account is authenticated)
- This is required before any `@rappstack/*` package can be published

**2. Package format mismatch**
The packages export `.ts` files directly (`"./index.ts"`) but the `files` field filters for `*.d.ts, *.js` — so TypeScript source would be excluded from the published tarball. Two options:

- **Option A (Simple, bun-only):** Update `files` to include `*.ts` and publish TypeScript source. Works with bun consumers but not standard Node.js/tsc.
- **Option B (Standard):** Add a build step (tsc or bun build) to compile `.ts` → `.js` + `.d.ts`, update exports to point to compiled output. Works universally.

Since all your apps use bun, **Option A** is the fastest path. We can always add a build step later.

**Once the org is created, the publish sequence is:**

Level 0 (no internal deps): domain--any--blog, domain--browser, domain--server, domain--server--auth, ui--any, ui--browser, ui--server
Level 1: domain--server--blog, domain--server--youtube, ui--any--blog
Level 2: ui--browser--blog, domain--browser--blog
Level 3: ui--server--blog, snippet--rappstack

Please create the @rappstack org on npm, then let me know which option (A or B) and I'll proceed with publishing.

I think @rappstack is created. Please check
<!-- /agent:exchange -->
