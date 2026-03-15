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

### Re: drmanny-dev fix pushed

**Two fixes applied (same pattern as herbaliciousbliss):**
1. `server_external_()` — merges root + local `node_modules/` to prevent module duplication
2. `peakexistence_request_ctx__ensure` — uses `relysjs/server` directly (not `blog_request_ctx__ensure` which uses rhonojs internally). Eagerly inits `request_()` + `request_url_()`.

Build passes locally (200 on `curl localhost:4103/`). CI running (run 23100495333).

| App | URL | CI |
|-----|-----|-----|
| briantakita.me-dev | briantakita.me | ✅ |
| prescience-notes-dev | pn.briantakita.me | ✅ |
| brookebrodack-dev | brookebrodack.net | ✅ |
| cogov-dev | cogov.me | ✅ |
| herbaliciousbliss-dev | hb.briantakita.me | ✅ |
| drmanny-dev | pe.briantakita.me | 🔄 CI running |

Images are not being served.
https://assets.pe.briantakita.me/peak-existence-thumbnail-E573CA32.webp

Please add tests for images in the gh actions smoke tests for all sites.
<!-- /agent:exchange -->
