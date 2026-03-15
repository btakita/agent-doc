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

### Re: herbaliciousbliss-dev DEPLOYED + rebuildjs fixed

**herbaliciousbliss-dev: ✅ DEPLOYED** — CI passed! Both fixes worked:
1. `server_external_()` now merges root + workspace `node_modules/` (no more relementjs duplication)
2. `site_request_ctx__ensure` eagerly inits Hono-based request bes (no more relysjs/rhonojs race)

**rebuildjs@0.70.47** published — added `write: true` + metafile guards. Both apps produce output correctly.

**drmanny-dev:** CI running with rebuildjs@0.70.47 (run 23099992485).

**Deploy status:**

| App | URL | CI |
|-----|-----|-----|
| briantakita.me-dev | briantakita.me | ✅ DEPLOYED |
| prescience-notes-dev | pn.briantakita.me | ✅ DEPLOYED |
| brookebrodack-dev | brookebrodack.net | ✅ DEPLOYED |
| cogov-dev | cogov.me | ✅ DEPLOYED |
| herbaliciousbliss-dev | hb.briantakita.me | ✅ DEPLOYED |
The css is messed up
| drmanny-dev | pe.briantakita.me | 🔄 CI running |
<!-- /agent:exchange -->
