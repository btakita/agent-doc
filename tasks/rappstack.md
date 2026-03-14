---
agent_doc_session: fce94c2a-2072-49c6-924b-144e96b540b7
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---


The @rappstack/* packages were recently published.
The apps used submodules from rappstack.
Now that @rappstack/* packages are published, the apps need to reference the @rappstack/* packages instead of using submodules.

Convert brookebrodack-dev, cogov-dev, censible-core, drmanny-dev, herbaliciousbliss-dev, prescience-notes-dev

Use yalc if possible. And deploy via github actions. The deployments must depend on the published npm packages...not yalc. yalc is only for development.

<!-- agent:exchange -->

### Re: drmanny-dev fixed

**Root causes (3 issues):**
1. **tailwindcss not resolved** — esbuild needs `conditions: ['style']` for TW v4 (same fix as herbaliciousbliss-dev)
2. **Build timeout** — `relysjs__ready__wait(300_000)` hangs when `watch: false`. Wrapped in watch check.
3. **unique_tags_ not found** — Published v0.1.1 had stale code. Republished `@rappstack/domain--any--blog@0.1.2`

**Build passes locally.** Committed and pushed — CI running.

**Deploy status:**

| App | URL | CI |
|-----|-----|-----|
| briantakita.me-dev | briantakita.me | ✅ DEPLOYED |
| prescience-notes-dev | pn.briantakita.me | ✅ DEPLOYED |
| brookebrodack-dev | brookebrodack.net | ✅ DEPLOYED |
| cogov-dev | cogov.me | ✅ DEPLOYED |
| drmanny-dev | pe.briantakita.me | 🔄 CI running |
| herbaliciousbliss-dev | hb.briantakita.me | ❌ relysjs/server resolution (pre-existing) |
I thought herbaliciousbliss-dev migrated to rhonojs.
<!-- /agent:exchange -->
