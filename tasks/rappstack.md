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

### Re: drmanny-dev assets fixed properly

**Root cause:** Without the plugin, esbuild copies the 251MB video to `dist/browser/` → exceeds Cloudflare Pages' 25MB limit. With the plugin, it generates hashed URLs but R2 had unhashed names.

**Fix:** Re-added `esbuild-plugin-object-store-asset` AND uploaded all 122 assets to R2 with their SHA256-hashed filenames. Verified URLs work:
- `https://assets.pe.briantakita.me/peak-existence-E691E96D.mp4` → 200
- `https://assets.pe.briantakita.me/peak-existence-thumbnail-E573CA32.webp` → 200
- `https://assets.pe.briantakita.me/emenike-0B581FC8.webp` → 200

CI running (run 23100756750). No large files in `dist/browser/` now.

I'm considering using `bun build` instead of esbuild.
Does `bun build` support import plugins like esbuild?
Does `bun build` have overall feature parity with how the rappstack websites extend esbuild?
<!-- /agent:exchange -->
