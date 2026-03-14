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
<!-- /agent:exchange -->
