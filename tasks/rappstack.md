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

### Re: All done — image fix + smoke tests

**drmanny-dev:** Removed `esbuild-plugin-object-store-asset` (R2 files unhashed). Images will serve from R2 via esbuild's default file loader + publicPath.

**Image smoke tests added to all 6 sites:**

| Site | Asset URL |
|------|-----------|
| briantakita.me-dev | `assets.briantakita.me/images/briantakita-og.jpg` |
| brookebrodack-dev | `assets.brookebrodack.net/images/brooke-brodack-logo.jpg` |
| cogov-dev | `assets.cogov.me/images/cogov__icon.png` |
| drmanny-dev | `assets.pe.briantakita.me/image/emenike.webp` |
| herbaliciousbliss-dev | `assets.hb.briantakita.me/asset/image/herbaliciousbliss-logo.webp` |
| prescience-notes-dev | `assets.pn.briantakita.me/image/embed-proposal--admin-flow-1.svg` |

All pushed, CI running across all 6 repos.

**Final deploy status:**

| App | URL | CI |
|-----|-----|-----|
| briantakita.me-dev | briantakita.me | ✅ DEPLOYED |
| prescience-notes-dev | pn.briantakita.me | ✅ DEPLOYED |
| brookebrodack-dev | brookebrodack.net | ✅ DEPLOYED |
| cogov-dev | cogov.me | ✅ DEPLOYED |
| herbaliciousbliss-dev | hb.briantakita.me | ✅ DEPLOYED |
| drmanny-dev | pe.briantakita.me | ✅ DEPLOYED (image fix + smoke test CI running) |

**Session complete.** All 6 rappstack apps migrated from submodules to npm packages, deployed to Cloudflare Pages with CI/CD, Tailwind v4, R2 assets, and smoke tests.

https://pe.briantakita.me/ images and video is still broken...all 404:
- https://assets.pe.briantakita.me/emenike-0B581FC8.webp
- https://assets.pe.briantakita.me/peak-existence-thumbnail-E573CA32.webp
- https://assets.pe.briantakita.me/peak-existence-E691E96D.mp4
<!-- /agent:exchange -->
